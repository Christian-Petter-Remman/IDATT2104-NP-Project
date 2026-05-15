<template>
  <canvas
    ref="canvasEl"
    width="640"
    height="640"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onMouseUp"
    @mouseleave="onMouseLeave"
  />
</template>

<script setup>
// 64×64 pixel canvas rendered on a 640×640 HTML canvas element (10 px per cell).
// Handles mouse painting with optimistic local updates and throttled cursor
// position reporting to the backend.
import { ref, watch, onMounted } from 'vue'
import { useCanvasStore } from '../stores/canvas.js'

const store = useCanvasStore()
const canvasEl = ref(null)
let painting = false
let lastCursorSend = 0
// Minimum ms between cursor position POST requests to avoid flooding the backend.
const CURSOR_THROTTLE_MS = 80

const CELL = 10  // pixels per canvas cell
const SIZE = 64  // canvas grid dimension (cells)

// Full redraw: background, painted pixels, grid lines, and peer cursors.
function render() {
  const canvas = canvasEl.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')

  ctx.fillStyle = '#111'
  ctx.fillRect(0, 0, SIZE * CELL, SIZE * CELL)

  for (const [key, color] of store.pixels) {
    const [x, y] = key.split(',').map(Number)
    const [r, g, b, a] = color
    ctx.fillStyle = `rgba(${r},${g},${b},${a / 255})`
    ctx.fillRect(x * CELL + 1, y * CELL + 1, CELL - 1, CELL - 1)
  }

  ctx.strokeStyle = '#2a2a2a'
  ctx.lineWidth = 1
  for (let i = 0; i <= SIZE; i++) {
    ctx.beginPath()
    ctx.moveTo(i * CELL + 0.5, 0)
    ctx.lineTo(i * CELL + 0.5, SIZE * CELL)
    ctx.stroke()
    ctx.beginPath()
    ctx.moveTo(0, i * CELL + 0.5)
    ctx.lineTo(SIZE * CELL, i * CELL + 0.5)
    ctx.stroke()
  }

  // Render remote peer cursors as yellow dots; skip the local client's own cursor.
  for (const [userId, pos] of store.cursors) {
    if (userId === store.clientId) continue
    ctx.fillStyle = 'rgba(255,255,0,0.7)'
    ctx.beginPath()
    ctx.arc(pos.x * CELL + CELL / 2, pos.y * CELL + CELL / 2, 4, 0, Math.PI * 2)
    ctx.fill()
  }
}

// Paint the cell under the cursor and apply an optimistic local update so the
// canvas reflects the change immediately without waiting for the WebSocket delta.
function paintAt(e) {
  const x = Math.min(SIZE - 1, Math.max(0, Math.floor(e.offsetX / CELL)))
  const y = Math.min(SIZE - 1, Math.max(0, Math.floor(e.offsetY / CELL)))
  store.paint(x, y, store.selectedColor)
  // Optimistic local render
  store.pixels.set(`${x},${y}`, store.selectedColor)
}

function onMouseDown(e) {
  painting = true
  paintAt(e)
}

// Rate-limit cursor updates to avoid excessive POST traffic on every mousemove.
function sendCursor(e) {
  if (document.visibilityState !== 'visible') return
  const now = Date.now()
  if (now - lastCursorSend < CURSOR_THROTTLE_MS) return
  lastCursorSend = now
  const x = Math.min(SIZE - 1, Math.max(0, Math.floor(e.offsetX / CELL)))
  const y = Math.min(SIZE - 1, Math.max(0, Math.floor(e.offsetY / CELL)))
  store.updateCursor(x, y)
}

function onMouseMove(e) {
  if (painting) paintAt(e)
  sendCursor(e)
}
function onMouseUp() { painting = false }
function onMouseLeave() { painting = false }

onMounted(render)
watch(store.pixels, render)
watch(store.cursors, render)
</script>

<style scoped>
canvas {
  display: block;
  cursor: crosshair;
  image-rendering: pixelated;
  border: 1px solid #333;
}
</style>
