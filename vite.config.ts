import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri serves the build from dist/ and, in development, expects the dev
// server on a fixed port (see src-tauri/tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**'] },
  },
  build: {
    target: ['es2021', 'chrome105', 'safari15'],
    outDir: 'dist',
  },
})
