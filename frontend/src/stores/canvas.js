import { defineStore } from 'pinia'

// WebSocket instance lives outside Pinia state — Vue's Proxy wrapping breaks
// the WebSocket internal `this instanceof WebSocket` checks.
let _ws = null

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
    apiPort: 8080,
    selectedColor: [0, 0, 0, 255],
  }),

  getters: {
    paletteColors: (state) => [...state.palette].map(s => JSON.parse(s)),
  },

  actions: {
    init(port) {
      if (port) this.apiPort = port
      this.connect()
    },

    connect() {
      if (_ws) return
      _ws = new WebSocket(`ws://${window.location.hostname}:${this.apiPort}/ws`)

      _ws.onopen = () => {
        this.connected = true
        // Fetch node info if not yet populated (connect may fire before init resolves)
        if (!this.nodeId) {
          fetch(`http://${window.location.hostname}:${this.apiPort}/api/node`)
            .then(r => r.json())
            .then(d => { this.nodeId = d.id; this.nodeAddr = d.addr })
            .catch(() => {})
        }
      }

      _ws.onmessage = (evt) => {
        const msg = JSON.parse(evt.data)
        if (msg.type === 'diff') {
          this._applyDiff(msg)
        } else {
          this._applySnapshot(msg)
        }
      }

      _ws.onclose = () => {
        this.connected = false
        _ws = null
        setTimeout(() => this.connect(), 3000)
      }

      _ws.onerror = () => _ws && _ws.close()
    },

    _applySnapshot(msg) {
      // pixels: [{x, y, color}] or object map
      this.pixels.clear()
      if (Array.isArray(msg.pixels)) {
        for (const p of msg.pixels) {
          this.pixels.set(`${p.x},${p.y}`, p.color)
        }
      } else if (msg.pixels && typeof msg.pixels === 'object') {
        // object keyed by "x,y" → [r,g,b,a]
        for (const [key, color] of Object.entries(msg.pixels)) {
          this.pixels.set(key, color)
        }
      }

      // palette: [[r,g,b,a], ...]
      this.palette.clear()
      for (const color of (msg.palette ?? [])) {
        this.palette.add(JSON.stringify(color))
      }

      // active_peers: [uuid, ...]
      this.activePeers.clear()
      for (const peer of (msg.active_peers ?? msg.users ?? [])) {
        this.activePeers.add(peer)
      }

      // paint_total from GCounter value or direct field
      this.paintTotal = msg.paint_total ?? msg.paint_counts?.value ?? 0

      // leaderboard
      this.leaderboard = msg.leaderboard ?? []
    },

    _applyDiff(msg) {
      if (msg.pixels) {
        for (const p of msg.pixels) {
          this.pixels.set(`${p.x},${p.y}`, p.color)
        }
      }
      if (msg.palette_add) {
        for (const color of msg.palette_add) {
          this.palette.add(JSON.stringify(color))
        }
      }
      if (msg.palette_rm) {
        for (const color of msg.palette_rm) {
          this.palette.delete(JSON.stringify(color))
        }
      }
      if (msg.cursors) {
        for (const c of msg.cursors) {
          this.cursors.set(c.user_id, { x: c.x, y: c.y })
        }
      }
      if (msg.active_peers) {
        this.activePeers.clear()
        for (const peer of msg.active_peers) {
          this.activePeers.add(peer)
        }
      }
      if (msg.paint_total != null) {
        this.paintTotal = msg.paint_total
      }
      if (msg.leaderboard) {
        this.leaderboard = msg.leaderboard
      }
    },

    async paint(x, y, color) {
      await fetch(`http://${window.location.hostname}:${this.apiPort}/api/canvas/paint`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ x, y, color }),
      }).catch(() => {})
    },

    async addColor(color) {
      await fetch(`http://${window.location.hostname}:${this.apiPort}/api/palette`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ color }),
      }).catch(() => {})
    },

    async removeColor(color) {
      await fetch(`http://${window.location.hostname}:${this.apiPort}/api/palette`, {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ color }),
      }).catch(() => {})
    },
  },
})
