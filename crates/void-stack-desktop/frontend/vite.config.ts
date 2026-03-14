import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],

  // Use relative paths so Tauri can resolve assets from the custom protocol
  // (tauri://localhost on macOS, https://tauri.localhost on Windows).
  // Without this, absolute paths like /assets/... fail and cause a white screen.
  base: './',

  // Tauri dev server config
  server: {
    port: 5173,
    strictPort: true,
  },
})
