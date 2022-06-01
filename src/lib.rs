use cid::Cid;

pub trait StateObject {
  fn load() -> Self;
  fn save(&self) -> Cid;
}
