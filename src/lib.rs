use cid::Cid;

pub trait StateObject {
  fn load() -> Self;
  fn save(&self) -> Cid;
}

/// A macro to abort concisely.
/// This should be part of the SDK as it's very handy.
#[macro_export]
macro_rules! abort {
  ($code:ident, $msg:literal $(, $ex:expr)*) => {
      fvm_sdk::vm::abort(
          fvm_shared::error::ExitCode::$code.value(),
          Some(format!($msg, $($ex,)*).as_str()),
      )
  };
}