<template>
  <div class="leaderboard">
    <h3>Leaderboard</h3>
    <ol>
      <li
        v-for="(entry, idx) in store.leaderboard"
        :key="entry.peer_id"
        :class="{ self: entry.peer_id === store.nodeId }"
      >
        <span class="rank">{{ idx + 1 }}</span>
        <span class="peer-id">{{ entry.peer_id.slice(0, 8) }}</span>
        <span class="pixels">{{ entry.pixels }}</span>
      </li>
    </ol>
    <div class="global-ops">Global lifetime ops: {{ store.paintTotal }}</div>
  </div>
</template>

<script setup>
// Sidebar panel ranking peers by pixel ownership count (last-write-wins).
// Highlights the local node's own entry. Data comes from the store's
// leaderboard field, updated on every canvas delta that changes pixel ownership.
import { useCanvasStore } from '../stores/canvas.js'
const store = useCanvasStore()
</script>

<style scoped>
.leaderboard h3 {
  margin: 0 0 8px;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 1px;
  color: #666;
}
ol {
  list-style: none;
  margin: 0 0 8px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
li {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  padding: 3px 6px;
  border-radius: 2px;
  font-family: 'Courier New', monospace;
  color: #aaa;
}
li.self {
  background: #2a3a2a;
  border-left: 2px solid #4f4;
  color: #e0e0e0;
}
.rank {
  color: #666;
  width: 14px;
  text-align: right;
  flex-shrink: 0;
}
.peer-id {
  flex: 1;
}
.pixels {
  color: #4af;
  width: 32px;
  text-align: right;
  flex-shrink: 0;
}
.global-ops {
  font-size: 11px;
  color: #666;
  border-top: 1px solid #2a2a2a;
  padding-top: 6px;
  margin-top: 4px;
}
</style>
