// TODO: TodoDocument — composite CRDT for the collaborative todo list
//
// struct TodoDocument {
//     items: ORSet<Uuid>,                       — live item IDs
//     text:  HashMap<Uuid, LWWRegister<String>>,
//     done:  HashMap<Uuid, LWWRegister<bool>>,
// }
//
// merge: merge ORSet, then merge each register for items in the union
