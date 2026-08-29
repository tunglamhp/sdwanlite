import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    // Dev-only: forward API + config-stream to the controller so the UI is
    // same-origin (no CORS). Point the target at your controller's bind addr.
    proxy: {
      '/healthz': 'http://127.0.0.1:8090',
      '/metrics': 'http://127.0.0.1:8090',
      '/api': 'http://127.0.0.1:8090',
      '/stream': { target: 'ws://127.0.0.1:8090', ws: true },
    },
  },
})
