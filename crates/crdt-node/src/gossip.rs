// TODO: gossip loop
// Every 5 seconds: pick up to 2 random peers, open TCP connection,
// exchange NodeSnapshot (serde_json), merge received state into local NodeState,
// broadcast updated state to WebSocket clients
