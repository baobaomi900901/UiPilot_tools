import { describe, expect, it } from 'vitest'

import { SnapshotBuilder } from '../src/package-policy.js'
import { validateSnapshot } from '../src/validate.js'
import { createWav } from './wav-fixture.js'

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
  it('reports Host 0.3.4 for a valid network Manifest', () => {
    const report = validateSnapshot(networkSnapshot(), 'network-package', 'windows')
    expect(report.valid).toBe(true)
    expect(report.target).toEqual({ platform: 'windows', hostVersion: '0.3.4', apiVersion: 1 })
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
