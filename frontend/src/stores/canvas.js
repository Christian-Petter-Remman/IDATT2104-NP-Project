import { defineStore } from 'pinia'

// WebSocket instance lives outside Pinia state — Vue's Proxy wrapping breaks
// the WebSocket internal `this instanceof WebSocket` checks.
let _ws = null

// Stable per-tab identity used for cursor ownership. Survives page refresh
// within the same tab via sessionStorage.
const _storedClientId = sessionStorage.getItem('canvas-client-id') ?? crypto.randomUUID()
sessionStorage.setItem('canvas-client-id', _storedClientId)

export const useCanvasStore = defineStore('canvas', {
  state: () => ({
    pixels: new Map(),          // "x,y" → [r,g,b,a]
    palette: new Set(),         // JSON.stringify([r,g,b,a]) strings — value equality
    cursors: new Map(),         // userId → { x, y }
    activePeers: new Set(),     // uuid strings
    paintTotal: 0,
    leaderboard: [],            // [{ peer_id, pixels }] sorted desc
    connected: false,
    nodeId: null,
    nodeAddr: null,
    selectedColor: [0, 0, 0, 255],
    clientId: _storedClientId,
  }),

  getters: {
    paletteColors: (state) => [...state.palette].map(s => JSON.parse(s)),
  },

  actions: {
    init() {
      this.connect()
    },

    async fetchNodeInfo() {
      try {
        const r = await fetch('/api/node')
        const d = await r.json()
        this.nodeId = d.id
        this.nodeAddr = d.addr
      } catch {
        setTimeout(() => this.fetchNodeInfo(), 3000)
      }
    },

    connect() {
      if (_ws && _ws.readyState <= WebSocket.OPEN) return
      _ws = null
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      _ws = new WebSocket(`${proto}//${location.host}/ws`)

      _ws.onopen = () => {
        this.connected = true
        if (!this.nodeId) this.fetchNodeInfo()
      }

      _ws.onmessage = (evt) => {
        const msg = JSON.parse(evt.data)
        // Backend wraps state in `{type, payload}`.
        const data = msg.payload ?? msg
        if (msg.type === 'snapshot') {
          this._applySnapshot(data)
        } else if (msg.type === 'delta') {
          this._applyDelta(data)
        } else {
          // Unknown type: don't silently apply as a snapshot — that
          // masked decode errors and stale frames in earlier versions.
          console.warn('[canvas] dropped WS message with unknown type:', msg.type)
        }
      }

      _ws.onclose = () => {
        this.connected = false
        _ws = null
        setTimeout(() => this.connect(), 3000)
      }

      _ws.onerror = () => _ws && _ws.close()
    },

    // Full canvas state. Replaces every collection wholesale.
    _applySnapshot(data) {
      this.pixels.clear()
      for (const [key, color] of Object.entries(data.pixels ?? {})) {
        this.pixels.set(key, color)
      }
      this.palette.clear()
      for (const color of (data.palette ?? [])) {
        this.palette.add(JSON.stringify(color))
      }
      this.activePeers.clear()
      for (const peer of (data.active_peers ?? [])) {
        this.activePeers.add(peer)
      }
      this.paintTotal = data.paint_total ?? 0
      this.leaderboard = data.leaderboard ?? []
      this.cursors.clear()
      for (const [userId, pos] of Object.entries(data.cursors ?? {})) {
        this.cursors.set(userId, { x: pos[0], y: pos[1] })
      }
    },

    // Sparse update from the backend's `CanvasDeltaView`.
    //
    // `pixels` carries only the cells that changed and is patched into
    // the existing Map (no clear). The other fields are present only
    // when their underlying CRDT changed — when present, replace the
    // whole collection (the backend ships the full derived view for
    // these to keep the projection simple).
    _applyDelta(data) {
      for (const [key, color] of Object.entries(data.pixels ?? {})) {
        this.pixels.set(key, color)
      }
      if (data.active_peers !== undefined) {
        this.activePeers.clear()
        for (const peer of data.active_peers) {
          this.activePeers.add(peer)
        }
      }
      if (data.palette !== undefined) {
        this.palette.clear()
        for (const color of data.palette) {
          this.palette.add(JSON.stringify(color))
        }
      }
      if (data.paint_total !== undefined) {
        this.paintTotal = data.paint_total
      }
      if (data.leaderboard !== undefined) {
        this.leaderboard = data.leaderboard
      }
      if (data.cursors !== undefined) {
        for (const [userId, pos] of Object.entries(data.cursors)) {
          this.cursors.set(userId, { x: pos[0], y: pos[1] })
        }
      }
    },

    async paint(x, y, color) {
      await fetch('/api/canvas/paint', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ x, y, color }),
      }).catch(() => {})
    },

    async updateCursor(x, y) {
      await fetch('/api/canvas/cursor', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ user_id: this.clientId, x, y }),
      }).catch(() => {})
    },

    async addColor(color) {
      await fetch('/api/palette', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ color }),
      }).catch(() => {})
    },

    async removeColor(color) {
      await fetch('/api/palette', {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ color }),
      }).catch(() => {})
    },
  },
})
