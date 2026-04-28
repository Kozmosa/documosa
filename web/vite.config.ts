import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    // Cherry Markdown ships the interactive editor as one monolithic ESM file.
    // It is loaded only after a document opens, so the initial app chunk stays small.
    chunkSizeWarningLimit: 6000,
  },
})
