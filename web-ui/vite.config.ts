import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/healthz': 'http://127.0.0.1:18080',
      '/metrics': 'http://127.0.0.1:18080',
      '/api': 'http://127.0.0.1:18080',
      '/stream': { target: 'ws://127.0.0.1:18080', ws: true },
    },
  },
})
