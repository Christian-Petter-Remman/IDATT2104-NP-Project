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
import { ref, watch, onMounted } from 'vue'
import { useCanvasStore } from '../stores/canvas.js'

const store = useCanvasStore()
const canvasEl = ref(null)
let painting = false

const CELL = 10
const SIZE = 64

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

  for (const [userId, pos] of store.cursors) {
    if (userId === store.nodeId) continue
    ctx.fillStyle = 'rgba(255,255,0,0.7)'
    ctx.beginPath()
    ctx.arc(pos.x * CELL + CELL / 2, pos.y * CELL + CELL / 2, 4, 0, Math.PI * 2)
    ctx.fill()
  }
}

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
function onMouseMove(e) {
  if (painting) paintAt(e)
}
function onMouseUp() { painting = false }
function onMouseLeave() { painting = false }

onMounted(render)
watch(() => store.pixels, render, { deep: false })
watch(() => store.cursors, render, { deep: false })
</script>

<style scoped>
canvas {
  display: block;
  cursor: crosshair;
  image-rendering: pixelated;
  border: 1px solid #333;
}
</style>
