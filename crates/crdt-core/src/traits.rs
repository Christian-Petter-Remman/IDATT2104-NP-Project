pub trait Crdt: Clone {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&self, other: &Self) -> Self;
}
