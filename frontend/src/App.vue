<template>
  <div id="app">
    <div v-if="store.nodeId && !store.connected" class="reconnect-banner">
      Reconnecting to node...
    </div>

    <div v-if="!store.nodeId" class="connecting">
      Connecting...
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
import { onMounted } from 'vue'
import { useCanvasStore } from './stores/canvas.js'
import NodeInfo from './components/NodeInfo.vue'
import PixelCanvas from './components/PixelCanvas.vue'
import ColorPicker from './components/ColorPicker.vue'
import PeerList from './components/PeerList.vue'
import Leaderboard from './components/Leaderboard.vue'

const store = useCanvasStore()
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

.connecting {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100vh;
  font-size: 14px;
  color: #666;
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
