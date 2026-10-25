import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

import { SnapshotBuilder } from '../src/package-policy.js'
import { validateSnapshot } from '../src/validate.js'
import { createWav } from './wav-fixture.js'

import { multiCommandManifest } from './multi-command-fixture.js'

function timerSnapshot(includeAlarm = true) {
  const manifest = {
    schemaVersion: 1,
    pluginId: 'com.example.timer',
    version: '1.0.0',
    apiVersion: 1,
    minimumHostVersion: '0.2.0',
    name: 'Timer',
    supportedPlatforms: ['windows', 'macos'],
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
  const builder = new SnapshotBuilder('directory')
  builder.addFile('plugin.json', Buffer.from(JSON.stringify(manifest)))
  builder.addFile('dist/runtime.js', Buffer.from('export {}'))
  builder.addFile('dist/window.html', Buffer.from('<!doctype html>'))
  if (includeAlarm) builder.addFile('assets/sounds/timer-alarm.wav', createWav(100))
  return builder.finish()
}

function networkSnapshot() {
  const manifest = {
    schemaVersion: 1,
    pluginId: 'com.example.network',
    version: '1.0.0',
    apiVersion: 1,
    minimumHostVersion: '0.3.2',
    name: 'Network',
    supportedPlatforms: ['windows'],
    command: {
      defaultName: 'network',
      activationMode: 'live',
      outputMode: 'mainResult',
      inputRequired: false,
    },
    runtime: { entry: 'dist/runtime.js' },
    network: { httpsHosts: ['api.example.com'] },
    permissions: ['network.https'],
    settings: [],
  }
  const builder = new SnapshotBuilder('directory')
  builder.addFile('plugin.json', Buffer.from(JSON.stringify(manifest)))
  builder.addFile('dist/runtime.js', Buffer.from('export {}'))
  return builder.finish()
}

describe('validateSnapshot', () => {
  it('reports Host 0.9.0 for a valid network Manifest', () => {
    const report = validateSnapshot(networkSnapshot(), 'network-package', 'windows')
    expect(report.valid).toBe(true)
    expect(report.target).toEqual({ platform: 'windows', hostVersion: '0.9.0', apiVersion: 1 })
  })

  it('accepts the complete Timer package on Windows', () => {
    const report = validateSnapshot(timerSnapshot(), 'timer-package', 'windows')
    expect(report.valid).toBe(true)
    expect(report.plugin?.pluginId).toBe('com.example.timer')
  })

  it('rejects Timer without the fixed alarm and rejects its Windows-only permission on macOS', () => {
    const missing = validateSnapshot(timerSnapshot(false), 'timer-package', 'windows')
    expect(missing.issues.map((issue) => issue.code)).toContain('RESOURCE_INVALID')
    const macos = validateSnapshot(timerSnapshot(), 'timer-package', 'macos')
    expect(macos.issues.map((issue) => issue.code)).toContain('PERMISSION_UNSUPPORTED')
  })

  it('rejects an undeclared alarm and missing declared entries', () => {
    const builder = new SnapshotBuilder('directory')
    const manifest = {
      schemaVersion: 1,
      pluginId: 'com.example.result',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.2.0',
      name: 'Result',
      supportedPlatforms: ['windows'],
      command: { defaultName: 'result', activationMode: 'submit', outputMode: 'mainResult', inputRequired: false },
      runtime: { entry: 'dist/runtime.js' },
      permissions: [],
      settings: [],
    }
    builder.addFile('plugin.json', Buffer.from(JSON.stringify(manifest)))
    builder.addFile('assets/sounds/timer-alarm.wav', createWav(100))
    const report = validateSnapshot(builder.finish(), 'result-package', 'windows')
    expect(report.issues.map((issue) => issue.code)).toEqual(expect.arrayContaining(['RESOURCE_INVALID', 'ENTRY_MISSING']))
  })
})


it('reports declared commands in order with fixed aggregate mode on production success and semantic failure', async () => {
  const fixtures = JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json'), 'utf8'))
  for (const name of ['two-commands', 'single-command', 'reordered', 'constructor', 'duplicate-name', 'empty']) {
    const fixture = fixtures.find((item: any) => item.name === name)
    const builder = new SnapshotBuilder('directory')
    builder.addFile('plugin.json', Buffer.from(JSON.stringify(fixture.manifest)))
    builder.addFile('dist/runtime.js', Buffer.from('export {}'))
    const report = validateSnapshot(builder.finish(), 'multi-package', 'windows')
    expect(report.valid, name).toBe(!['duplicate-name', 'empty'].includes(name))
    expect(report.target.hostVersion).toBe('0.9.0')
    if (fixture.schemaValid) {
      expect(report.plugin).toEqual({ pluginId: fixture.manifest.pluginId, version: fixture.manifest.version,
        outputMode: 'mainResult', commands: fixture.manifest.commands.map((command: any) => ({
          commandId: command.id, defaultName: command.defaultName, activationMode: command.activationMode, outputMode: 'mainResult',
        })),
      })
      expect(report.issues.map((issue) => issue.code)).not.toContain('API_INCOMPATIBLE')
    } else expect(report.plugin).toBeUndefined()
  }
})



it('forwards command identities while leaving shared failures unowned and preserving phase order', () => {
  const value = multiCommandManifest()
  value.minimumHostVersion = '0.9.1'
  value.commands[0].inputRequired = true
  value.commands[1].summary = ''
  value.permissions = ['clipboard.read']
  const builder = new SnapshotBuilder('directory')
  builder.addFile('plugin.json', Buffer.from(JSON.stringify(value)))
  const report = validateSnapshot(builder.finish(), 'multi', 'macos')
  expect(report.plugin?.commands?.map(command => command.commandId)).toEqual(['upper', 'constructor'])
  expect(report.issues.map(issue => issue.code)).toEqual([
    'MANIFEST_SEMANTIC_INVALID', 'MANIFEST_SEMANTIC_INVALID', 'PLATFORM_INCOMPATIBLE', 'API_INCOMPATIBLE', 'PERMISSION_UNSUPPORTED', 'ENTRY_MISSING',
  ])
  expect(report.issues.slice(0, 2).map(issue => [issue.commandId, issue.location?.value])).toEqual([
    ['upper', '/commands/0/inputPlaceholder'], ['constructor', '/commands/1/summary'],
  ])
  expect(report.issues.slice(2).every(issue => !Object.hasOwn(issue, 'commandId'))).toBe(true)
})

it('caps mixed command/resource findings at the same smallest 100 keys with stable phases', () => {
  const value = multiCommandManifest()
  value.minimumHostVersion = '0.9.1'
  value.commands[0].summary = ''
  value.commands[1].inputRequired = true
  const builder = new SnapshotBuilder('directory')
  builder.addFile('plugin.json', Buffer.from(JSON.stringify(value)))
  builder.addFile('icon.png', Buffer.from('bad'))
  for (let index = 104; index >= 0; index -= 1) builder.addFile(`dist/${String(index).padStart(3, '0')}.css`, Buffer.from('@import "missing.css";'))
  const report = validateSnapshot(builder.finish(), 'multi', 'windows')
  expect(report.truncated).toBe(true)
  expect(report.issues).toHaveLength(100)
  expect(report.issues.slice(0, 5).map(issue => issue.code)).toEqual([
    'MANIFEST_SEMANTIC_INVALID', 'MANIFEST_SEMANTIC_INVALID', 'API_INCOMPATIBLE', 'ENTRY_MISSING', 'ICON_INVALID',
  ])
  expect(report.issues[5]?.path).toBe('dist/000.css')
  expect(report.issues.at(-1)?.path).toBe('dist/094.css')
})
