use uuid::Uuid;

pub type NodeId = Uuid;
pub type Rgba = (u8, u8, u8, u8);

pub trait Crdt: Clone {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&self, other: &Self) -> Self;
}
