import { resolve } from 'node:path'
import { defineConfig } from 'vite'

export default defineConfig(({ mode }) => ({
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
