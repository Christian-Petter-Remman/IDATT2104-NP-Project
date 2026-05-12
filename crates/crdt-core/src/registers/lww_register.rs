// TODO: LWWRegister<T> — last-write-wins register
// Fields: value: T, timestamp: u64 (unix ms), node_id: Uuid (tie-break)
// merge: keep entry with higher timestamp; node_id breaks ties deterministically
