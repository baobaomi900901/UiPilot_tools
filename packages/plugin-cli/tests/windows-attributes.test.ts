import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

type ProbeChild = EventEmitter & { stdin: PassThrough; stdout: PassThrough; stderr: PassThrough; kill: ReturnType<typeof vi.fn> }
const state = vi.hoisted(() => ({ children: [] as ProbeChild[], attributes: 0x20, reply: true, malformed: false }))
vi.mock('node:fs/promises', () => ({
  realpath: async (path: string) => path,
  lstat: async (path: string) => ({
    isDirectory: () => !path.endsWith('.exe'), isFile: () => path.endsWith('.exe'), isSymbolicLink: () => false,
  }),
}))
vi.mock('node:child_process', () => ({
  spawn: vi.fn(() => {
    const child = Object.assign(new EventEmitter(), {
      stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(),
      kill: vi.fn(),
    })
    child.stdin.on('data', (bytes: Buffer) => {
      if (!state.reply) return
      const request = JSON.parse(bytes.toString())
      const response = state.malformed ? '{}' : JSON.stringify({
        entries: request.paths.map((path: string) => ({ path, attributes: state.attributes })),
      })
      queueMicrotask(() => child.stdout.write(response + '\n'))
    })
    child.stdin.on('finish', () => queueMicrotask(() => { child.emit('exit', 0); child.emit('close', 0) }))
    state.children.push(child)
    return child
  }),
}))

import { createWindowsAttributeProbe, withWindowsAttributeProbe } from '../src/windows-attributes.js'

beforeEach(() => { state.children = []; state.attributes = 0x20; state.reply = true; state.malformed = false })
afterEach(() => { vi.useRealTimers(); vi.unstubAllEnvs() })
const environment = { SystemRoot: 'C:\\Windows', WINDIR: 'C:\\Windows' }

it('reuses one process but reads fresh attributes on every request', async () => {
  const probe = createWindowsAttributeProbe(environment)
  try {
    expect((await probe('D:\\package', ['file'])).get('file')).toBe(0x20)
    state.attributes = 0x400
    expect((await probe('D:\\package', ['file'])).get('file')).toBe(0x400)
    expect(state.children).toHaveLength(1)
  } finally { probe.close?.() }
})

it('closes the owned process explicitly and rejects subsequent requests', async () => {
  const probe = createWindowsAttributeProbe(environment)
  await probe('D:\\package', [''])
  probe.close()
  expect(state.children[0].kill).toHaveBeenCalledOnce()
  await expect(probe('D:\\package', [''])).rejects.toMatchObject({ code: 'SOURCE_INVALID' })
})

it('fails closed on an incomplete response and terminates the process', async () => {
  state.malformed = true
  const probe = createWindowsAttributeProbe(environment)
  await expect(probe('D:\\package', ['file'])).rejects.toMatchObject({ code: 'SOURCE_INVALID' })
  expect(state.children[0].kill).toHaveBeenCalledOnce()
})

it('times out and terminates an unresponsive process', async () => {
  vi.useFakeTimers()
  state.reply = false
  const probe = createWindowsAttributeProbe(environment)
  const check = expect(probe('D:\\package', ['file'])).rejects.toMatchObject({ code: 'SOURCE_INVALID' })
  await vi.advanceTimersByTimeAsync(5001)
  await check
  expect(state.children[0].kill).toHaveBeenCalledOnce()
})

it.each([false, true])('releases its process when the scoped operation fails: %s', async (fail) => {
  vi.stubEnv('SystemRoot', environment.SystemRoot)
  vi.stubEnv('WINDIR', environment.WINDIR)
  const operation = withWindowsAttributeProbe('win32', undefined, async (probe) => {
    await probe!('D:\\package', ['file'])
    if (fail) throw new Error('read failed')
    return 'done'
  })
  if (fail) await expect(operation).rejects.toThrow('read failed')
  else await expect(operation).resolves.toBe('done')
  expect(state.children[0].kill).toHaveBeenCalledOnce()
})

it('rejects an oversized response and kills the process', async () => {
  state.reply = false
  const probe = createWindowsAttributeProbe(environment)
  const check = expect(probe('D:\\package', ['file'])).rejects.toMatchObject({ code: 'SOURCE_INVALID' })
  await vi.waitFor(() => expect(state.children).toHaveLength(1))
  state.children[0].stdout.write(Buffer.alloc(256 * 1024 + 1))
  await check
  expect(state.children[0].kill).toHaveBeenCalledOnce()
})

it('rejects a pending request if the process exits', async () => {
  state.reply = false
  const probe = createWindowsAttributeProbe(environment)
  const check = expect(probe('D:\\package', ['file'])).rejects.toMatchObject({ code: 'SOURCE_INVALID' })
  await vi.waitFor(() => expect(state.children).toHaveLength(1))
  state.children[0].emit('exit', 1)
  await check
})
