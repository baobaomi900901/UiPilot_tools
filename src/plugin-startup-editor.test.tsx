// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { beforeEach, expect, it, vi } from 'vitest'
import { StartupCommandEditor } from './plugin-startup-editor'

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }))
})

it.each(['rename', 'favorite'] as const)('keeps the pending %s gate across an unrelated authoritative revision', async (kind) => {
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host)
  let reject!: (reason: Error) => void
  const pending = new Promise<void>((_accept, fail) => { reject = fail })
  const operations = { name: vi.fn(async (_value: string | null) => {}), favorite: vi.fn(async (_value: boolean) => {}), hotkey: vi.fn(async (_value: string | null) => {}) }
  if (kind === 'rename') operations.name.mockReturnValueOnce(pending)
  else operations.favorite.mockReturnValueOnce(pending)
  const reload = vi.fn(async (_accept?: (revision: string) => void) => {})
  const command = { commandId: 'one', defaultName: 'one', effectiveName: 'one', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }
  const render = async (revision: string) => act(async () => root.render(<StartupCommandEditor command={command} single operations={operations} revision={revision} reload={reload} />))
  try {
    await render('1:1')
    const input = host.querySelector<HTMLInputElement>('[aria-label="启动键"]')!
    if (kind === 'rename') {
      await act(async () => {
        input.focus()
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '/draft')
        input.dispatchEvent(new Event('input', { bubbles: true }))
      })
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    } else await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="设为常用"]')!.click())
    await render('1:2')
    expect(input.readOnly).toBe(true)
    expect(host.querySelector<HTMLButtonElement>('[aria-label="设为常用"]')!.disabled).toBe(true)
    await act(async () => {
      host.querySelector<HTMLButtonElement>('[aria-label="恢复默认启动键"]')!.click()
      host.querySelector<HTMLButtonElement>('[aria-label="设为常用"]')!.click()
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })
    expect(operations.name).toHaveBeenCalledTimes(kind === 'rename' ? 1 : 0)
    expect(operations.favorite).toHaveBeenCalledTimes(kind === 'favorite' ? 1 : 0)
    await act(async () => reject(new Error('old outcome')))
    expect(reload).toHaveBeenCalledOnce()
    expect(input.readOnly).toBe(false)
    expect(host.querySelector('[role="alert"]')).toBeNull()
    if (kind === 'rename') expect(input.value).toBe('/draft')
  } finally { await act(async () => root.unmount()); host.remove() }
})

it('retains a failed draft when the authoritative read reports a different durable command', async () => {
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host)
  const initial = { commandId: 'one', defaultName: 'one', effectiveName: 'one', nameOverride: null as string | null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }
  const operations = { name: vi.fn(async (_value: string | null) => { throw new Error('unknown write result') }), favorite: vi.fn(async (_value: boolean) => {}), hotkey: vi.fn(async (_value: string | null) => {}) }
  const render = (command = initial, revision = '1') => root.render(<StartupCommandEditor command={command} single operations={operations} revision={revision} reload={async (accept) => {
    accept?.('2'); render({ ...initial, effectiveName: 'durable-other', nameOverride: 'durable-other' }, '2')
  }} />)
  try {
    await act(async () => render())
    const input = host.querySelector<HTMLInputElement>('[aria-label="启动键"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '/my-draft')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    expect(input.value).toBe('/my-draft')
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('无法保存')
    expect(operations.name).toHaveBeenCalledOnce()
  } finally { await act(async () => root.unmount()); host.remove() }
})
