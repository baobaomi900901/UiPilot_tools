import { resolve } from 'node:path'
import { defineConfig } from 'vite'

export default defineConfig(({ mode }) => ({
  server: {
    host: '127.0.0.1',
    port: 14321,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/target/**'],
    },
  },
  build: {
    rollupOptions:
      mode === 'security-probe'
        ? {
            input: {
              main: resolve(import.meta.dirname, 'index.html'),
              securityProbe: resolve(import.meta.dirname, 'security-probe.html'),
            },
          }
        : undefined,
  },
}))
