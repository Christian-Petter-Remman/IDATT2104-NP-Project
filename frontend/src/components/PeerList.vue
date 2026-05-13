<template>
  <div class="peer-list">
    <h3>Peers</h3>
    <div class="status" :class="store.connected ? 'online' : 'offline'">
      <span class="dot"></span>
      {{ store.connected ? 'Connected' : 'Reconnecting...' }}
    </div>
    <div class="peer-count">{{ store.activePeers.size }} online</div>
    <ul>
      <li
        v-for="peer in [...store.activePeers]"
        :key="peer"
        :class="{ self: peer === store.nodeId }"
      >
        {{ peer.slice(0, 8) }}<span v-if="peer === store.nodeId"> (you)</span>
      </li>
    </ul>
    <div class="paint-total">Total ops: {{ store.paintTotal }}</div>
  </div>
</template>

<script setup>
import { useCanvasStore } from '../stores/canvas.js'
const store = useCanvasStore()
</script>

<style scoped>
.peer-list h3 {
  margin: 0 0 8px;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 1px;
  color: #666;
}
.status {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  margin-bottom: 6px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.online .dot { background: #4f4; }
.offline .dot { background: #f44; }
.online { color: #4f4; }
.offline { color: #f44; }
.peer-count {
  font-size: 12px;
  color: #666;
  margin-bottom: 6px;
}
ul {
  list-style: none;
  margin: 0 0 8px;
  padding: 0;
}
li {
  font-size: 12px;
  padding: 2px 0;
  font-family: 'Courier New', monospace;
  color: #aaa;
}
li.self {
  color: #4af;
}
.paint-total {
  font-size: 12px;
  color: #666;
  border-top: 1px solid #2a2a2a;
  padding-top: 6px;
  margin-top: 4px;
}
</style>
