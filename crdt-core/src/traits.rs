use uuid::Uuid;

pub type NodeId = Uuid;

pub trait Crdt {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&mut self, other: Self);
}
