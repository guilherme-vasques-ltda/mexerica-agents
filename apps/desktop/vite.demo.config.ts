import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const desktop = resolve(dirname(fileURLToPath(import.meta.url)))

export default defineConfig({
  plugins: [react()],
  root: resolve(desktop, 'src/renderer/demo'),
  build: {
    outDir: resolve(desktop, 'dist/demo/desktop'),
    emptyOutDir: true,
    rollupOptions: { input: resolve(desktop, 'src/renderer/demo/index.html') },
  },
  resolve: { alias: { '@shared': resolve(desktop, 'src/shared') } },
})
