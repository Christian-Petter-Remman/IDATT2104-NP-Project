// TODO: MVRegister<T> — multi-value register using vector clocks
// Stores Vec<(VectorClock, T)>; on merge keep values with incomparable clocks
// On write: increment own clock entry, replace all dominated entries
