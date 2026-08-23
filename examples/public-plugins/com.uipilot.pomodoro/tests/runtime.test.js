import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const idle = (revision = '1') => Object.freeze({
  timerRevision: revision,
  phase: 'idle',
  durationMs: null,
  remainingMs: null,
})

const deferred = () => {
  let resolve
  let reject
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

const fakeTimer = (initialState = idle()) => {
  const calls = []
  let handler = null
  return {
    calls,
    emit(next) {
      handler?.(next)
    },
    async getState() {
      calls.push(['getState'])
      return initialState
    },
    async start(input) {
      calls.push(['start', input])
      const durationMs = input?.durationMs ?? initialState.durationMs
      return Object.freeze({
        timerRevision: (BigInt(initialState.timerRevision) + 1n).toString(),
        phase: 'running',
        durationMs,
        remainingMs: initialState.remainingMs ?? durationMs,
      })
    },
    async stop() {
      calls.push(['stop'])
      return initialState
    },
    async reset() {
      calls.push(['reset'])
      return idle('2')
    },
    onStateChanged(next) {
      calls.push(['subscribe'])
      handler = next
      return () => {
        calls.push(['unsubscribe'])
        if (handler === next) handler = null
      }
    },
  }
}

test('manifest declares the host-owned Pomodoro contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json', packageRoot), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.pomodoro')
  assert.equal(manifest.version, '1.2.0')
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
  assert.deepEqual((await readdir(packageRoot)).sort(), ['assets', 'dist', 'icon.png', 'plugin.json'])
  assert.deepEqual((await readdir(new URL('assets/', packageRoot))).sort(), ['sounds'])
  assert.deepEqual((await readdir(new URL('assets/sounds/', packageRoot))).sort(), [
    'timer-alarm.wav',
  ])
  assert.deepEqual((await readdir(new URL('dist/', packageRoot))).sort(), [
    'runtime.js',
    'window.css',
    'window.html',
    'window.js',
  ])
  const alarm = await readFile(new URL('assets/sounds/timer-alarm.wav', packageRoot))
  assert.equal(alarm.subarray(0, 4).toString('ascii'), 'RIFF')
  assert.equal(alarm.subarray(8, 12).toString('ascii'), 'WAVE')
  assert.equal(alarm.readUInt32LE(4) + 8, alarm.length)
})

test('runtime supplies only the invocation-derived completion message', async () => {
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
    data: { completionMessage: '回到工作' },
  })
})

test('window renders the exact duration selector and default ten minutes', async () => {
  const html = await readFile(new URL('../package/dist/window.html', import.meta.url), 'utf8')
  const options = [...html.matchAll(/<option value="(\d+)">([^<]+)<\/option>/g)]
    .map((match) => [Number(match[1]), match[2]])
  assert.deepEqual(options, [
    [10, '10分钟'],
    [15, '15分钟'],
    [25, '25分钟'],
    [30, '30分钟'],
    [45, '45分钟'],
  ])
  assert.match(html, /<select id="duration"[^>]*disabled/)
  assert.match(html, /<output id="time"[^>]*>10:00<\/output>/)
})

test('window model restores only legal stored durations and reads in parallel after subscribing', async () => {
  const { createPomodoroModel } = await import(new URL('../package/dist/window.js', import.meta.url))
  for (const [stored, expected, persisted] of [
    [25, 25, 25],
    [null, 10, null],
    [20, 10, null],
    ['25', 10, null],
  ]) {
    const calls = []
    const timer = fakeTimer()
    const originalSubscribe = timer.onStateChanged
    const originalGet = timer.getState
    timer.onStateChanged = (handler) => {
      calls.push('subscribe')
      return originalSubscribe.call(timer, handler)
    }
    timer.getState = async () => {
      calls.push('timer-get')
      return originalGet.call(timer)
    }
    const model = createPomodoroModel({
      timer,
      storage: {
        async get() {
          calls.push('storage-get')
          return stored
        },
        async set() {},
      },
      completionMessage: '完成',
      now: () => 0,
    })

    await model.initialize()

    assert.equal(calls[0], 'subscribe')
    assert.deepEqual(new Set(calls.slice(1, 3)), new Set(['timer-get', 'storage-get']))
    assert.equal(model.getSnapshot().effectiveDurationMinutes, expected)
    assert.equal(model.getSnapshot().persistedDurationMinutes, persisted)
    assert.equal(model.getSnapshot().durationReadPending, false)
    model.dispose()
  }
})

test('read and save failures use fixed recovery behavior', async () => {
  const { createPomodoroModel } = await import(new URL('../package/dist/window.js', import.meta.url))
  const readFailure = createPomodoroModel({
    timer: fakeTimer(),
    storage: {
      async get() { throw new Error('read') },
      async set() {},
    },
    completionMessage: '完成',
    now: () => 0,
  })
  await readFailure.initialize()
  assert.equal(readFailure.getSnapshot().effectiveDurationMinutes, 10)
  assert.equal(readFailure.getSnapshot().persistedDurationMinutes, null)
  assert.equal(readFailure.getSnapshot().error, '无法读取计时长度')

  const saveFailure = createPomodoroModel({
    timer: fakeTimer(),
    storage: {
      async get() { return 15 },
      async set() { throw new Error('save') },
    },
    completionMessage: '完成',
    now: () => 0,
  })
  await saveFailure.initialize()
  await saveFailure.selectDuration(25)
  assert.equal(saveFailure.getSnapshot().effectiveDurationMinutes, 15)
  assert.equal(saveFailure.getSnapshot().persistedDurationMinutes, 15)
  assert.equal(saveFailure.getSnapshot().pendingDurationMinutes, null)
  assert.equal(saveFailure.getSnapshot().error, '无法保存计时长度')
})

test('pending save blocks a new round and successful save supplies the next duration', async () => {
  const { createPomodoroModel } = await import(new URL('../package/dist/window.js', import.meta.url))
  const save = deferred()
  const timer = fakeTimer()
  const model = createPomodoroModel({
    timer,
    storage: {
      async get() { return 10 },
      async set() { return save.promise },
    },
    completionMessage: '休息结束',
    now: () => 0,
  })
  await model.initialize()

  const selection = model.selectDuration(25)
  assert.equal(model.getSnapshot().pendingDurationMinutes, 25)
  assert.equal(model.getSnapshot().selectedDurationMinutes, 25)
  await model.start()
  assert.equal(timer.calls.filter(([name]) => name === 'start').length, 0)

  save.resolve()
  await selection
  await model.start()
  assert.deepEqual(timer.calls.find(([name]) => name === 'start'), [
    'start',
    { durationMs: 1_500_000, completionMessage: '休息结束' },
  ])
})

test('paused resume remains argument-free while a next-round duration save is pending', async () => {
  const { createPomodoroModel } = await import(new URL('../package/dist/window.js', import.meta.url))
  const save = deferred()
  const paused = Object.freeze({
    timerRevision: '8',
    phase: 'paused',
    durationMs: 600_000,
    remainingMs: 420_000,
  })
  const timer = fakeTimer(paused)
  const model = createPomodoroModel({
    timer,
    storage: {
      async get() { return 10 },
      async set() { return save.promise },
    },
    completionMessage: '完成',
    now: () => 0,
  })
  await model.initialize()
  const revision = model.getSnapshot().timerState.timerRevision

  const selection = model.selectDuration(45)
  await model.start()

  assert.deepEqual(timer.calls.find(([name]) => name === 'start'), ['start', undefined])
  assert.equal(model.getSnapshot().timerState.durationMs, 600_000)
  assert.notEqual(model.getSnapshot().timerState.timerRevision, revision)
  save.resolve()
  await selection
})

test('disposed views ignore late reads, saves, timer completions, and queued callbacks', async () => {
  const { createPomodoroModel } = await import(new URL('../package/dist/window.js', import.meta.url))
  const oldRead = deferred()
  const oldBaseline = deferred()
  let oldHandler = null
  const oldModel = createPomodoroModel({
    timer: {
      onStateChanged(handler) { oldHandler = handler; return () => {} },
      getState() { return oldBaseline.promise },
      async start() { return idle('99') },
      async stop() { return idle('99') },
      async reset() { return idle('99') },
    },
    storage: {
      get() { return oldRead.promise },
      async set() {},
    },
    completionMessage: '旧视图',
    now: () => 0,
  })
  const oldInitialization = oldModel.initialize()
  oldModel.dispose()

  const lateSave = deferred()
  const lateTimer = deferred()
  let staleHandler = null
  const stalePaused = Object.freeze({
    timerRevision: '7', phase: 'paused', durationMs: 600_000, remainingMs: 300_000,
  })
  const staleModel = createPomodoroModel({
    timer: {
      onStateChanged(handler) { staleHandler = handler; return () => {} },
      async getState() { return stalePaused },
      start() { return lateTimer.promise },
      async stop() { return stalePaused },
      async reset() { return idle('8') },
    },
    storage: {
      async get() { return 10 },
      set() { return lateSave.promise },
    },
    completionMessage: '过期操作',
    now: () => 0,
  })
  await staleModel.initialize()
  const staleSaveCompletion = staleModel.selectDuration(25)
  const staleTimerCompletion = staleModel.start()
  staleModel.dispose()

  const currentModel = createPomodoroModel({
    timer: fakeTimer(idle('2')),
    storage: { async get() { return 15 }, async set() {} },
    completionMessage: '当前视图',
    now: () => 0,
  })
  await currentModel.initialize()
  const currentBefore = currentModel.getSnapshot()

  oldRead.resolve(45)
  oldBaseline.resolve(Object.freeze({
    timerRevision: '98', phase: 'running', durationMs: 2_700_000, remainingMs: 1,
  }))
  oldHandler?.(Object.freeze({
    timerRevision: '99', phase: 'running', durationMs: 2_700_000, remainingMs: 0,
  }))
  lateSave.resolve()
  lateTimer.resolve(Object.freeze({
    timerRevision: '8', phase: 'running', durationMs: 600_000, remainingMs: 299_000,
  }))
  staleHandler?.(Object.freeze({
    timerRevision: '9', phase: 'running', durationMs: 600_000, remainingMs: 298_000,
  }))
  await oldInitialization
  await Promise.all([staleSaveCompletion, staleTimerCompletion])

  assert.deepEqual(currentModel.getSnapshot(), currentBefore)
})

test('content delegates timing to the host and never creates local authority', async () => {
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
