// TODO: ORSet<T> — observed-remove set
// Each add generates a unique tag; remove moves all of element's tags to tombstones
// Element is live if it has at least one tag not in tombstones
// merge: union element-tag maps; union tombstone sets
