<template>
  <div class="color-picker">
    <h3>Palette</h3>
    <div class="swatches">
      <div
        v-for="color in store.paletteColors"
        :key="JSON.stringify(color)"
        class="swatch"
        :class="{ selected: isSelected(color) }"
        :style="{ background: toHex(color) }"
        :title="toHex(color)"
        @click="selectColor(color)"
        @contextmenu.prevent="store.removeColor(color)"
      />
    </div>
    <div class="add-row">
      <input
        type="color"
        v-model="colorValue"
        class="color-input"
        @input="previewColor"
      />
      <button class="add-btn" @click="addColor">+</button>
    </div>
    <div class="hint">Right-click swatch to remove</div>
  </div>
</template>

<script setup>
import { ref } from 'vue'
import { useCanvasStore } from '../stores/canvas.js'

const store = useCanvasStore()
const colorValue = ref('#ff0000')

function toHex([r, g, b]) {
  return '#' + [r, g, b].map(v => v.toString(16).padStart(2, '0')).join('')
}

function hexToRgba(hex) {
  const clean = hex.replace('#', '')
  if (clean.length !== 6) return null
  const r = parseInt(clean.slice(0, 2), 16)
  const g = parseInt(clean.slice(2, 4), 16)
  const b = parseInt(clean.slice(4, 6), 16)
  if (isNaN(r) || isNaN(g) || isNaN(b)) return null
  return [r, g, b, 255]
}

function isSelected(color) {
  return JSON.stringify(color) === JSON.stringify(store.selectedColor)
}

function selectColor(color) {
  store.selectedColor = color
}

function previewColor() {
  const rgba = hexToRgba(colorValue.value)
  if (rgba) store.selectedColor = rgba
}

function addColor() {
  const rgba = hexToRgba(colorValue.value)
  if (rgba) store.addColor(rgba)
}
</script>

<style scoped>
.color-picker h3 {
  margin: 0 0 8px;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 1px;
  color: #666;
}
.swatches {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-bottom: 8px;
}
.swatch {
  width: 28px;
  height: 28px;
  border-radius: 2px;
  cursor: pointer;
  border: 2px solid transparent;
  box-sizing: border-box;
}
.swatch:hover {
  border-color: #888;
}
.swatch.selected {
  border-color: #fff;
  outline: 1px solid #4af;
}
.add-row {
  display: flex;
  gap: 4px;
  align-items: center;
}
.color-input {
  width: 40px;
  height: 28px;
  padding: 0;
  border: 1px solid #2a2a2a;
  border-radius: 2px;
  background: #0f0f0f;
  cursor: pointer;
}
.color-input:focus {
  outline: none;
  border-color: #4af;
}
.add-btn {
  background: #2a2a2a;
  border: 1px solid #333;
  color: #e0e0e0;
  width: 28px;
  height: 28px;
  cursor: pointer;
  border-radius: 2px;
  font-size: 16px;
  line-height: 1;
}
.add-btn:hover {
  background: #3a3a3a;
}
.hint {
  font-size: 10px;
  color: #444;
  margin-top: 4px;
}
</style>
