<template>
  <div class="node-info">
    <span class="label">Node:</span>
    <span class="value">{{ store.nodeId ? store.nodeId.slice(0, 8) : '...' }}</span>
    <span class="label">Addr:</span>
    <span class="value">{{ store.nodeAddr ?? '...' }}</span>
  </div>
</template>

<script setup>
import { useCanvasStore } from '../stores/canvas.js'
const store = useCanvasStore()
</script>

<style scoped>
.node-info {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}
.label {
  color: #666;
}
.value {
  color: #4af;
  font-family: 'Courier New', monospace;
}
</style>
