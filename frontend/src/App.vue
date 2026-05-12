<template>
  <div id="app">
    <div v-if="!store.connected" class="reconnect-banner">
      Reconnecting to node...
    </div>

    <div v-if="!store.nodeId" class="port-config">
      <label>
        API Port:
        <input v-model.number="portInput" type="number" class="port-input" />
      </label>
      <button class="connect-btn" @click="init">Connect</button>
    </div>

    <div v-else class="layout">
      <header class="topbar">
        <NodeInfo />
      </header>
      <main class="main">
        <PixelCanvas />
      </main>
      <aside class="sidebar">
        <ColorPicker />
        <PeerList />
        <Leaderboard />
      </aside>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { useCanvasStore } from './stores/canvas.js'
import NodeInfo from './components/NodeInfo.vue'
import PixelCanvas from './components/PixelCanvas.vue'
import ColorPicker from './components/ColorPicker.vue'
import PeerList from './components/PeerList.vue'
import Leaderboard from './components/Leaderboard.vue'

const store = useCanvasStore()
const portInput = ref(8080)

function init() {
  store.init(portInput.value)
}

onMounted(() => store.init())
</script>

<style>
*, *::before, *::after { box-sizing: border-box; }
body {
  margin: 0;
  background: #0f0f0f;
  color: #e0e0e0;
  font-family: 'Courier New', monospace;
}

.reconnect-banner {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 100;
  background: #c03030;
  color: #fff;
  text-align: center;
  padding: 8px;
  font-size: 14px;
  font-weight: bold;
}

.port-config {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 40px;
  font-size: 14px;
}
.port-input {
  background: #1a1a1a;
  border: 1px solid #333;
  color: #e0e0e0;
  padding: 4px 8px;
  font-family: 'Courier New', monospace;
  width: 80px;
  margin-left: 6px;
  border-radius: 2px;
}
.connect-btn {
  background: #2a2a2a;
  border: 1px solid #4af;
  color: #4af;
  padding: 4px 14px;
  cursor: pointer;
  border-radius: 2px;
  font-family: 'Courier New', monospace;
}
.connect-btn:hover {
  background: #1a3040;
}

.layout {
  display: grid;
  grid-template-rows: 40px 1fr;
  grid-template-columns: 1fr 240px;
  grid-template-areas:
    "topbar topbar"
    "main   sidebar";
  min-height: 100vh;
}

.topbar {
  grid-area: topbar;
  display: flex;
  align-items: center;
  padding: 0 16px;
  background: #1a1a1a;
  border-bottom: 1px solid #2a2a2a;
}

.main {
  grid-area: main;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 24px;
}

.sidebar {
  grid-area: sidebar;
  background: #1a1a1a;
  border-left: 1px solid #2a2a2a;
  padding: 16px 12px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 20px;
}
</style>
