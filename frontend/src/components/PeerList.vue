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

    <div class="connect-form">
      <h3>Connect to peer</h3>
      <input
        v-model="peerAddr"
        class="peer-input"
        placeholder="192.168.x.x:9090"
        @keyup.enter="connect"
      />
      <button class="connect-btn" @click="connect">Connect</button>
      <div v-if="connectMsg" :class="['connect-msg', connectStatus]">{{ connectMsg }}</div>
    </div>
  </div>
</template>

<script setup>
import { ref } from 'vue'
import { useCanvasStore } from '../stores/canvas.js'

const store = useCanvasStore()

const peerAddr = ref('')
const connectStatus = ref('')
const connectMsg = ref('')

async function connect() {
  const addr = peerAddr.value.trim()
  if (!addr) return
  const result = await store.bootstrap(addr)
  if (result === 'ok') {
    connectStatus.value = 'ok'
    connectMsg.value = 'Submitted'
    peerAddr.value = ''
  } else {
    connectStatus.value = 'err'
    connectMsg.value = result === 'network-error' ? 'Server unreachable' : 'Invalid address'
  }
  setTimeout(() => { connectMsg.value = ''; connectStatus.value = '' }, 2000)
}
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
.connect-form {
  margin-top: 12px;
  border-top: 1px solid #2a2a2a;
  padding-top: 10px;
}
.peer-input {
  width: 100%;
  background: #111;
  border: 1px solid #333;
  color: #e0e0e0;
  font-family: 'Courier New', monospace;
  font-size: 12px;
  padding: 4px 6px;
  box-sizing: border-box;
  margin-bottom: 6px;
}
.peer-input:focus {
  outline: none;
  border-color: #555;
}
.connect-btn {
  width: 100%;
  background: #222;
  border: 1px solid #444;
  color: #ccc;
  font-family: 'Courier New', monospace;
  font-size: 12px;
  padding: 4px;
  cursor: pointer;
}
.connect-btn:hover {
  background: #2a2a2a;
  border-color: #666;
}
.connect-msg {
  margin-top: 4px;
  font-size: 11px;
  font-family: 'Courier New', monospace;
}
.connect-msg.ok { color: #4f4; }
.connect-msg.err { color: #f44; }
</style>
