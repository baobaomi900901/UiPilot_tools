import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

import { validateManifest } from '../src/manifest.js'

function timerManifest(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    pluginId: 'com.example.timer',
    version: '1.0.0',
    apiVersion: 1,
    minimumHostVersion: '0.2.0',
    name: 'Timer',
    supportedPlatforms: ['windows'],
    command: {
      defaultName: 'timer',
      activationMode: 'submit',
      outputMode: 'window',
      inputRequired: false,
    },
    runtime: { entry: 'dist/runtime.js' },
    window: { entry: 'dist/window.html' },
    permissions: ['ui.window', 'notifications.publish', 'timer.control'],
    settings: [],
  }
}

function validate(value: unknown, platform: 'windows' | 'macos' = 'windows') {
  return validateManifest(Buffer.from(JSON.stringify(value)), platform)
}

describe('validateManifest', () => {
  it('accepts the current pomodoro manifest on Windows', async () => {
    const bytes = await readFile(
      resolve('../../examples/public-plugins/com.uipilot.pomodoro/package/plugin.json'),
    )
    const result = validateManifest(bytes, 'windows')
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.manifest.permissions).toContain('timer.control')
  })

  it.each([
    ['live activation', (value: any) => (value.command.activationMode = 'live')],
    ['main result', (value: any) => (value.command.outputMode = 'mainResult')],
    ['missing window', (value: any) => delete value.window],
    ['missing ui.window', (value: any) => (value.permissions = value.permissions.filter((x: string) => x !== 'ui.window'))],
    ['missing notifications.publish', (value: any) => (value.permissions = value.permissions.filter((x: string) => x !== 'notifications.publish'))],
  ])('rejects timer.control with %s', (_name, mutate) => {
    const value = timerManifest()
    mutate(value)
    expect(validate(value)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
  })

  it('accepts a legal panel package and rejects illegal panel combinations', () => {
    const value = {
      schemaVersion: 1,
      pluginId: 'com.example.panel',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.3.0',
      name: 'Panel',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'panel',
        activationMode: 'submit',
        outputMode: 'panel',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      panel: { entry: 'dist/panel.html' },
      permissions: ['ui.panel'],
      settings: [],
    }
    expect(validate(value)).toMatchObject({ ok: true })

    const missingPermission = structuredClone(value)
    missingPermission.permissions = []
    expect(validate(missingPermission)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const missingEntry = structuredClone(value)
    delete (missingEntry as { panel?: unknown }).panel
    expect(validate(missingEntry)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const withTimer = structuredClone(value)
    withTimer.permissions = ['ui.panel', 'notifications.publish', 'timer.control']
    expect(validate(withTimer)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const tooNewHost = structuredClone(value)
    tooNewHost.minimumHostVersion = '0.4.0'
    expect(validate(tooNewHost)).toMatchObject({
      ok: false,
      issues: [{ code: 'API_INCOMPATIBLE' }],
    })
  })

  it('still accepts older non-panel packages that declare minimumHostVersion 0.2.0', () => {
    const value = timerManifest()
    expect(validate(value)).toMatchObject({ ok: true })
  })

  it('rejects timer.control for macOS even when the manifest lists both platforms', () => {
    const value = timerManifest()
    value.supportedPlatforms = ['windows', 'macos']
    expect(validate(value, 'macos')).toMatchObject({
      ok: false,
      issues: [{ code: 'PERMISSION_UNSUPPORTED' }],
    })
  })

  it('distinguishes JSON, Schema, semantic, platform, API, and permission failures', () => {
    expect(validateManifest(Buffer.from('{"x":1,"x":2}'), 'windows')).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_JSON_INVALID' }],
    })

    const schema = timerManifest()
    ;(schema as any).unknown = true
    expect(validate(schema)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SCHEMA_INVALID' }] })

    const semantic = timerManifest()
    ;(semantic as any).pluginId = 'Invalid ID'
    expect(validate(semantic)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })

    const platform = timerManifest()
    const platformResult = validate(platform, 'macos')
    expect(platformResult.ok).toBe(false)
    if (!platformResult.ok) {
      expect(platformResult.issues.map((issue) => issue.code)).toEqual([
        'PLATFORM_INCOMPATIBLE',
        'PERMISSION_UNSUPPORTED',
      ])
    }

    const api = timerManifest()
    ;(api as any).apiVersion = 2
    expect(validate(api)).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })

    const permission = timerManifest()
    ;(permission as any).permissions = ['ui.window', 'network.https']
    expect(validate(permission)).toMatchObject({ ok: false, issues: [{ code: 'PERMISSION_UNSUPPORTED' }] })
  })

  it('validates numeric and select setting boundaries', () => {
    const number = timerManifest()
    ;(number as any).settings = [{ type: 'number', key: 'limit', label: 'Limit', min: 10, max: 5 }]
    expect(validate(number)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })

    const select = timerManifest()
    ;(select as any).settings = [{ type: 'select', key: 'mode', label: 'Mode', options: [] }]
    expect(validate(select)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
  })
})
