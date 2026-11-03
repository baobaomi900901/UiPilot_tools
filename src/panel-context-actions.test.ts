// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { describe, it, expect, vi } from 'vitest'
import { createContextActionsMenu } from './context-actions-ui.js'

function setup() {
  document.body.innerHTML = '<textarea>example draft</textarea>'
  let epoch = '1'
  const api = { setStatus: vi.fn().mockResolvedValue(undefined), focusHostInput: vi.fn().mockResolvedValue(undefined) }
  const target: any = { uipilotPluginPanel: api }
  const invoke = vi.fn().mockResolvedValue(undefined)
  const install = new Function(readFileSync('src/panel-context-actions.js', 'utf8').replace('export function', 'function') + ';return installPanelContextActions')()
  const bridge = install({ document, window: target, getEpoch: () => epoch, invoke, createMenu: createContextActionsMenu })
  return { api, invoke, bridge, open: target.__UIPILOT_PLUGIN_PANEL_ACTIONS__, setEpoch(value: string) { epoch = value } }
}

describe('Panel contextual actions', () => {
  it('captures focus before opening and isolates keys from plugin shortcuts', () => {
    const t = setup()
    const editor = document.querySelector('textarea')!
    editor.focus(); editor.setSelectionRange(2, 5)
    const run = vi.fn()
    t.bridge.register(({ source }: any) => {
      expect(source).toBe('content'); expect(document.activeElement).toBe(editor)
      return { context: [{ id: 'save', title: '保存', run }] }
    })
    const key = new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, cancelable: true })
    expect(t.bridge.handleKey(key)).toBe(true)
    expect(key.defaultPrevented).toBe(true)
    t.bridge.handleKey(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }))
    expect(run).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(editor)
    expect([editor.selectionStart, editor.selectionEnd]).toEqual([2, 5])
  })

  it('restores Host input on cancel and submits through one captured ticket on execute', async () => {
    const t = setup()
    t.bridge.register(() => ({}))
    t.open({ sessionEpoch: '1', source: 'hostInput', ticket: 'one' })
    t.bridge.handleKey(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }))
    expect(t.api.focusHostInput).toHaveBeenCalledOnce()
    expect(t.invoke).not.toHaveBeenCalled()
    t.open({ sessionEpoch: '1', source: 'hostInput', ticket: 'two' })
    const search = document.querySelector<HTMLButtonElement>('.uipilot-context-action')!
    search.click(); search.click()
    await Promise.resolve()
    expect(t.invoke).toHaveBeenCalledExactlyOnceWith('plugin_panel_actions_submit', { sessionEpoch: '1', ticket: 'two' })
    t.bridge.reset()
  })

  it('invalidates old menu callbacks on reset or epoch replacement and rejects invalid providers', () => {
    const t = setup()
    const run = vi.fn()
    const unsubscribe = t.bridge.register(() => ({ context: [{ id: 'edit', title: '编辑', run }] }))
    t.open({ sessionEpoch: '1' })
    const old = document.querySelector<HTMLButtonElement>('.uipilot-context-action')!
    t.bridge.reset(); old.click()
    t.open({ sessionEpoch: '1' }); t.setEpoch('2')
    document.querySelector<HTMLButtonElement>('.uipilot-context-action')!.click()
    expect(run).not.toHaveBeenCalled()
    unsubscribe()
    t.bridge.register(() => ({ context: [{ id: 'bad', title: 'Bad' }] }))
    t.open({ sessionEpoch: '2' })
    expect(t.api.setStatus).toHaveBeenCalledWith({ message: '操作未完成，请重试', tone: 'error' })
    expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  })
})

it('offers search for Host input and return-to-input only for content focus', () => {
  const t = setup()
  t.bridge.register(() => ({}))
  t.open({ sessionEpoch: '1', source: 'hostInput', ticket: 'search' })
  expect(document.querySelector('.uipilot-context-actions')?.textContent).toContain('搜索')
  expect(document.querySelector('.uipilot-context-actions')?.textContent).not.toContain('返回主输入框')
  t.bridge.reset()
  t.open({ sessionEpoch: '1', source: 'content' })
  const labels = [...document.querySelectorAll('.uipilot-context-action')].map(node => node.textContent)
  expect(labels.filter(label => label?.includes('返回主输入框'))).toHaveLength(1)
  expect(labels.some(label => label?.includes('搜索'))).toBe(false)
  t.bridge.reset()
})

it('keeps only the plugin return-to-input action when content declares Ctrl+F', () => {
  const t = setup()
  const run = vi.fn()
  t.bridge.register(() => ({ global: [{ id: 'notes-input', title: '返回主输入框', shortcut: 'Ctrl+F', run }] }))
  t.open({ sessionEpoch: '1', source: 'content' })
  const rows = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')]
  expect(rows.filter(row => row.textContent?.includes('返回主输入框'))).toHaveLength(1)
  rows[0].click()
  expect(run).toHaveBeenCalledOnce()
  t.bridge.reset()
})
