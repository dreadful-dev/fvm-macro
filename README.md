# FVM Macros
## `#[derive(StateObject)]`
This macro derives the `StateObject` trait implementation for the annotated struct. This trait handles state serde from the blockstore via `load` and `save` methods.

```rs
#[derive(Serialize_tuple, Deserialize_tuple, Clone, Debug, Default, StateObject)]
pub struct ComputeState {
    pub count: u64,
}
```

Generates the following:

```rs
impl StateObject for ComputeState {
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
```

## `#[fvm_actor(state=ComputeState, dispatch="method_num")]`

This procedural macro derives an `Actor` trait implementation for the annotated implementation. The implementation is annotated so the macro can parse the public methods and autmatically create an impl with dispatch and the actor entrypoint.

```rs
#[fvm_actor(state=ComputeState, dispatch="method_num")]
impl ComputeActor {
    /// The constructor populates the initial state.
    ///
    /// Method num 1. This is part of the Filecoin calling convention.
    /// InitActor#Exec will call the constructor on method_num = 1.

    pub fn constructor(_: RawBytes, state: ComputeState) -> Option<RawBytes> {
      // This constant should be part of the SDK.
      const INIT_ACTOR_ADDR: ActorID = 1;

      // Should add SDK sugar to perform ACL checks more succinctly.
      // i.e. the equivalent of the validate_* builtin-actors runtime methods.
      // https://github.com/filecoin-project/builtin-actors/blob/master/actors/runtime/src/runtime/fvm.rs#L110-L146
      if sdk::message::caller() != INIT_ACTOR_ADDR {
          abort!(USR_FORBIDDEN, "constructor invoked by non-init actor");
      }

      state.save();
      None
    }

    pub fn say_hello(_: RawBytes, mut state: ComputeState) -> Option<RawBytes> {
      state.count += 1;
      state.save();
  
      let ret = to_vec(format!("Hello world #{}!", &state.count).as_str());
      match ret {
          Ok(ret) => Some(RawBytes::new(ret)),
          Err(err) => {
              abort!(
                  USR_ILLEGAL_STATE,
                  "failed to serialize return value: {:?}",
                  err
              );
          }
      }
    }
}
```

Each public method is extracted and assigned a number, because of this, the constructor needs to be the first public function in the impl. 

Example code generated from the macro:

```rs
impl Actor for ComputeActor {
      fn load() -> ComputeState {
        match sdk::message::method_number() {
          1 => <ComputeState>::default(),
          _ => <ComputeState>::load()
        }
      }
      fn dispatch(id: u32) -> u32 {
        let params = sdk::message::params_raw(id).unwrap().1;
        let params = RawBytes::new(params);
        let state: ComputeState = <ComputeActor>::load();

        let ret: Option<RawBytes> = match sdk::message::method_number() {
          1 => <ComputActor>::constructor(params, state),
          2 => <ComputActor>::say_hello(params, state),
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

    #[no_mangle]
    pub fn invoke(id: u32) -> u32 {
        <ComputeActor>::dispatch(id)
    }
  };
```