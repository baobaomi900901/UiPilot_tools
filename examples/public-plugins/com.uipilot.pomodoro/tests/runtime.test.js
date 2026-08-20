import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)

test('manifest declares the host-owned Pomodoro contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json', packageRoot), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.pomodoro')
  assert.equal(manifest.command.defaultName, 'pomodoro')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'window')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, [
    'ui.window',
    'notifications.publish',
    'timer.control',
  ])
})

test('package contains only the declared Pomodoro assets', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual((await readdir(new URL('dist/', packageRoot))).sort(), [
    'runtime.js',
    'window.css',
    'window.html',
    'window.js',
  ])
})

test('runtime supplies ten seconds and a completion message without starting a timer', async () => {
  const runtime = await import(new URL('../package/dist/runtime.js', import.meta.url))
  const invocation = Object.freeze({
    apiVersion: 1,
    requestId: 'pomodoro-request-1',
    input: '回到工作',
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-20T23:00:00+08:00',
    }),
  })
  const forbiddenApi = new Proxy({}, { get: () => { throw new Error('runtime must not control timer') } })
  assert.deepEqual(await runtime.onCommand(invocation, forbiddenApi), {
    requestId: 'pomodoro-request-1',
    data: { initialDurationMs: 10_000, completionMessage: '回到工作' },
  })
})

test('window model orders revisions and renders the initial ten seconds', async () => {
  const model = await import(new URL('../package/dist/window.js', import.meta.url))
  assert.equal(model.formatDuration(10_000), '00:10')
  const revision9 = Object.freeze({
    timerRevision: '9', phase: 'running', durationMs: 10_000, remainingMs: 9_000,
  })
  const revision10 = Object.freeze({
    timerRevision: '10', phase: 'paused', durationMs: 10_000, remainingMs: 8_000,
  })
  assert.deepEqual(model.mergeTimerState(revision9, revision10), revision10)
  assert.equal(model.mergeTimerState(revision10, revision9), revision10)
  const refreshed = Object.freeze({ ...revision10, phase: 'running', remainingMs: 7_000 })
  const running = Object.freeze({ ...revision10, phase: 'running', remainingMs: 8_000 })
  assert.equal(model.mergeTimerState(running, refreshed), running)
  assert.deepEqual(model.mergeTimerState(running, refreshed, true), refreshed)
})

test('content subscribes before reading and delegates all control to the host timer', async () => {
  const source = await readFile(new URL('../package/dist/window.js', import.meta.url), 'utf8')
  const subscribe = source.indexOf('timer.onStateChanged')
  const read = source.indexOf('timer.getState()')
  assert.ok(subscribe >= 0 && subscribe < read)
  for (const required of ['timer.start(', 'timer.stop()', 'timer.reset()', 'performance.now()']) {
    assert.ok(source.includes(required), `missing ${required}`)
  }
  for (const forbidden of ['Notification(', 'notifications.schedule', 'notifications.publish', 'fetch(']) {
    assert.ok(!source.includes(forbidden), `forbidden local authority: ${forbidden}`)
  }
})
