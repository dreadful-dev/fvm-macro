use proc_macro;
use proc_macro2;

use quote::{quote, format_ident};
use syn;

struct ParseError;

#[derive(Default, Debug)]
struct FvmActorMacroAttributes {
  state: String,
  dispatch_method: String,
  invoke: bool
}

#[proc_macro_derive(StateObject)]
pub fn fvm_state_macro_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Construct a representation of Rust code as a syntax tree
    // that we can manipulate
    let ast = syn::parse(input).unwrap();

    // Build the trait implementation
    impl_fvm_state_macro(&ast)
}

fn impl_fvm_state_macro(ast: &syn::DeriveInput) -> proc_macro::TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl StateObject for #name {
          fn load() -> Self {
              // First, load the current state root.
              let root = match sdk::sself::root() {
                  Ok(root) => root,
                  Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get root: {:?}", err),
              };
      
              // Load the actor state from the state tree.
              match Blockstore.get_cbor::<Self>(&root) {
                  Ok(Some(state)) => state,
                  Ok(None) => abort!(USR_ILLEGAL_STATE, "state does not exist"),
                  Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get state: {}", err),
              }
          }
          fn save(&self) -> Cid {
            let serialized = match to_vec(self) {
                Ok(s) => s,
                Err(err) => abort!(USR_SERIALIZATION, "failed to serialize state: {:?}", err),
            };
            let cid = match sdk::ipld::put(Code::Blake2b256.into(), 32, DAG_CBOR, serialized.as_slice())
            {
                Ok(cid) => cid,
                Err(err) => abort!(USR_SERIALIZATION, "failed to store initial state: {:}", err),
            };
            if let Err(err) = sdk::sself::set_root(&cid) {
                abort!(USR_ILLEGAL_STATE, "failed to set root ciid: {:}", err);
            }
            cid
          }  
        }
    };
    gen.into()
}

#[proc_macro_attribute]
pub fn fvm_actor(attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = proc_macro2::TokenStream::from(item);
  let clone = input.clone();

  check_impl(&clone);
  let macro_attributes = parse_attributes(attr.to_string());
  let (name, fns) = meta(&clone);
  impl_fvm_actor(macro_attributes, name, fns, input)
}

fn impl_fvm_actor(macro_attributes: FvmActorMacroAttributes, name: proc_macro2::TokenTree, fns: Vec<String>, original_stream: proc_macro2::TokenStream) -> proc_macro::TokenStream {
  let arms = fns.iter().enumerate().map(|(i, x)| match_arm(i+1, &x, &name)).collect::<Vec<_>>();
  let state_class = format_ident!("{}", macro_attributes.state);
  let mut invoke_block = quote! {};

  if macro_attributes.invoke != false {
    invoke_block = quote! {
      #[no_mangle]
      pub fn invoke(id: u32) -> u32 {
          <#name>::dispatch(id)
      }
    };
  }

  let gen = quote!{
    #original_stream

    pub trait Actor { 
      fn dispatch(id: u32) -> u32; 
      fn load() -> #state_class;
    }

    impl Actor for #name {
      fn load() -> #state_class {
        match sdk::message::method_number() {
          1 => <#state_class>::default(),
          _ => <#state_class>::load()
        }
      }
      fn dispatch(id: u32) -> u32 {
        let params = sdk::message::params_raw(id).unwrap().1;
        let params = RawBytes::new(params);
        let state: #state_class = <#name>::load();

        let ret: Option<RawBytes> = match sdk::message::method_number() {
          #(#arms)*
          _ => abort!(USR_UNHANDLED_MESSAGE, "unrecognized method"),
        };

        match ret {
          None => NO_DATA_BLOCK_ID,
          Some(v) => match sdk::ipld::put_block(DAG_CBOR, v.bytes()) {
              Ok(id) => id,
              Err(err) => abort!(USR_SERIALIZATION, "failed to store return value: {}", err),
          },
        }
      }
    }

    #invoke_block
  };

  gen.into()
}

fn match_arm(i: usize, fn_name: &String, class_name: &proc_macro2::TokenTree) -> proc_macro2::TokenStream {
  let fn_name = format_ident!("{}", fn_name);
  let lit = proc_macro2::Literal::usize_unsuffixed(i);
  quote! { #lit => <#class_name>::#fn_name(params, state), }
}

fn check_impl (t: &proc_macro2::TokenStream) {
  let stream = t.clone();
  let mut iter = stream.into_iter();

  let first = iter.next().unwrap();
  iter.next();
  let third = iter.next().unwrap();

  let first_ident = extract_identifier(&first);
  let third_ident = extract_identifier(&third);

  if first_ident != "impl" {
    panic!("fvm_actor: this macro can only be used on struct impl blocks.");
  }
  if third_ident == "for" {
    panic!("fvm_actor: this macro does not support trait impl definitions, sorry!");
  }
}

fn extract_identifier(tt: &proc_macro2::TokenTree) -> String {
  let r = match tt {
    proc_macro2::TokenTree::Ident(i) => Ok(i.to_string()),
    _ => Err(ParseError)
  };

  r.unwrap_or_default()
}

fn meta(ts: &proc_macro2::TokenStream) -> (proc_macro2::TokenTree, Vec<String>) {
  let mut item_iter = ts.clone().into_iter();
  let _impl = item_iter.next().unwrap();
  let name = item_iter.next().unwrap();
  let group = item_iter.next().unwrap();
  let fns = extract_pub_fns(&group);
  (name, fns)
}

fn extract_pub_fns(tt: &proc_macro2::TokenTree) -> Vec<String> {
  let mut v: Vec<String> = vec![];
  let mut current: String = "{}".to_owned();
  let mut previous: String = "{}".to_owned();

  match tt {
    proc_macro2::TokenTree::Group(g) => {
      let gi = g.stream().into_iter();
      for g in gi {
        if previous == "pub" && current == "fn" {
          v.push(extract_identifier(&g));
        }

        previous = current;
        current = extract_identifier(&g);
      }
    },
    _ => ()
  }

  v
}

fn parse_attributes(attr_string: String) -> FvmActorMacroAttributes {
  let mut attrs = FvmActorMacroAttributes::default();
  
  // invoke by default
  attrs.invoke = true;

  let vec = attr_string
    .split(",")
    .into_iter()
    .map(|x| x.to_string())
    .collect::<Vec<String>>()
    .into_iter()
    .map(|x: String| x.replace("\"", "")
      .split(" = ")
      .into_iter()
      .map(|x| x.trim().to_string())
      .collect::<Vec<String>>())
    .collect::<Vec<Vec<String>>>();
  
  for i in vec {
    match i[0].as_str() {
      "state" => {
        attrs.state = i[1].to_string();
      },
      "dispatch" => {
        attrs.dispatch_method = i[1].to_string();
      },
      "invoke" => {
        attrs.invoke = i[1].parse().unwrap_or_default();
      }
      _ => {}
    }
  }

  println!("{:?}", attrs);

  attrs
}