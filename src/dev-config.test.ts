import { describe, expect, it } from 'vitest'

// @ts-expect-error Vite supplies the raw source module in Vitest.
import packageJsonSource from '../package.json?raw'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import startupScript from '../scripts/dev-with-everything.ps1?raw'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import tauriConfigSource from '../src-tauri/tauri.conf.json?raw'
import viteConfig from '../vite.config'

describe('development server contract', () => {
  it('uses one strict project-specific IPv4 endpoint for Vite and Tauri', async () => {
    const config =
      typeof viteConfig === 'function'
        ? await viteConfig({
            command: 'serve',
            mode: 'development',
            isSsrBuild: false,
            isPreview: false,
          })
        : await viteConfig
    const server = config.server

    expect(server).toMatchObject({
      host: '127.0.0.1',
      port: 14321,
      strictPort: true,
    })
    expect(JSON.parse(tauriConfigSource).build.devUrl).toBe(
      `http://${server?.host}:${server?.port}`,
    )
  })

  it('keeps command-line launchers from overriding the Vite endpoint', () => {
    const packageJson = JSON.parse(packageJsonSource)

    expect(packageJson.scripts['dev:vite']).toBe('vite')
    expect(startupScript).not.toMatch(/--(?:host|port|strictPort)/)
  })

  it('disables native maximize only for the fixed-size main window', () => {
    const windows = JSON.parse(tauriConfigSource).app.windows as Array<Record<string, unknown>>
    const main = windows.find((window) => window.label === 'main')
    const find = windows.find((window) => window.label === 'find')

    expect(main).toMatchObject({ resizable: false, maximizable: false, fullscreen: false })
    expect(find).toMatchObject({ resizable: true, fullscreen: false })
    expect(find).not.toHaveProperty('maximizable')
  })
})
