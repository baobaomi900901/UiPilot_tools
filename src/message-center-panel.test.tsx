// @vitest-environment jsdom

import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { describe, expect, it, vi } from 'vitest'

import { MessageCenterPanel } from './message-center-panel'
import type { MessageCenterStateSnapshot, MessageView } from './protocol'

function message(id: string, content = `message-${id}`, icon = false): MessageView {
  return {
    id: id as MessageView['id'],
    pluginId: 'com.uipilot.demo-win',
    pluginNameSnapshot: 'Demo Window',
    pluginIconUrl: icon
      ? 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png'
      : null,
    createdAt: '2026-08-19T01:02:03.000Z',
    content,
    readAt: null,
  }
}

function state(
  overrides: Partial<MessageCenterStateSnapshot> = {},
): MessageCenterStateSnapshot {
  return {
    status: 'ready',
    unreadCount: 0,
    summaryRevision: '2' as MessageCenterStateSnapshot['summaryRevision'],
    snapshotRevision: '2' as MessageCenterStateSnapshot['snapshotRevision'],
    messages: [],
    clearPending: false,
    operationError: false,
    ...overrides,
  }
}

async function renderPanel(value: MessageCenterStateSnapshot, onClear = vi.fn()) {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  await act(async () => root.render(<MessageCenterPanel state={value} onClear={onClear} />))
  return {
    host,
    onClear,
    async rerender(next: MessageCenterStateSnapshot) {
      await act(async () => root.render(<MessageCenterPanel state={next} onClear={onClear} />))
    },
    async unmount() {
      await act(async () => root.unmount())
      host.remove()
    },
  }
}

describe('MessageCenterPanel', () => {
  it('renders newest-first stable rows as plain text with current icons and local time', async () => {
    const unsafeText = '<b>plain & literal</b>'
    const mounted = await renderPanel(state({ messages: [message('1', unsafeText), message('2', 'newest', true)] }))
    const rows = [...mounted.host.querySelectorAll<HTMLElement>('.message-center-row')]

    expect(rows.map((row) => row.querySelector('.message-center-content')?.textContent)).toEqual([
      'newest',
      unsafeText,
    ])
    expect(rows[1]!.querySelector('b')).toBeNull()
    expect(rows[0]!.querySelector('.plugin-icon-32')).toBeTruthy()
    expect(rows[1]!.querySelector('.plugin-icon-fallback:not([hidden])')).toBeTruthy()
    expect(rows[0]!.querySelector('time')?.textContent).toBe(new Date('2026-08-19T01:02:03.000Z').toLocaleString())
    await mounted.unmount()
  })

  it('distinguishes empty and unavailable without exposing a retry action', async () => {
    const mounted = await renderPanel(state())
    expect(mounted.host.textContent).toContain('暂无消息')
    expect(mounted.host.textContent).not.toContain('消息不可用')

    await mounted.rerender(state({ status: 'unavailable', unreadCount: undefined }))
    expect(mounted.host.textContent).toContain('消息不可用，请重启 UiPilot')
    expect(mounted.host.textContent).not.toContain('暂无消息')
    expect([...mounted.host.querySelectorAll('button')].some((button) => button.textContent === '重试')).toBe(false)
    expect(mounted.host.querySelector<HTMLButtonElement>('button')?.disabled).toBe(true)
    await mounted.unmount()
  })

  it('exposes one clear command while preserving rows during pending and recoverable failure', async () => {
    const onClear = vi.fn()
    const value = state({ messages: [message('1')] })
    const mounted = await renderPanel(value, onClear)
    const clear = () => [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('清空全部'))!

    await act(async () => clear().click())
    expect(onClear).toHaveBeenCalledOnce()
    await mounted.rerender(state({ messages: value.messages, clearPending: true }))
    expect(clear().disabled).toBe(true)
    expect(mounted.host.querySelectorAll('.message-center-row')).toHaveLength(1)

    await mounted.rerender(state({ messages: value.messages, operationError: true }))
    expect(mounted.host.textContent).toContain('MessageOperationFailed')
    expect(mounted.host.querySelectorAll('.message-center-row')).toHaveLength(1)
    await mounted.unmount()
  })
})
