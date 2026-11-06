// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { ShareSurface } from './plugin-share'
import type { PluginShareView } from './protocol'

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

it.each([['ArrowDown', 0], ['ArrowUp', 2]] as const)('initial %s selects the correct receiver among three rows', async (key, index) => {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  const select = vi.fn()
  const share: PluginShareView = {
    transferId: '1', sourceEpoch: '7', phase: 'choosing', target: null, targetLabel: null, error: null, receivers: [0, 1, 2].map(i => ({
      pluginId: `com.example.receiver${i}`, receiverId: `draft${i}`, pluginName: `Receiver ${i}`,
      commandId: 'default', title: `Draft ${i}`, maxBytes: 16384, disabledReason: null,
    })),
  }
  try {
    await act(async () => root.render(<ShareSurface share={share} select={select} close={() => {}} />))
    const input = host.querySelector('input')!
    expect(host.querySelector('[aria-selected="true"]')).toBeNull()
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })))
    expect(host.querySelector('[aria-selected="true"]')?.textContent).toContain(`Draft ${index}`)
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })))
    expect(select).toHaveBeenCalledExactlyOnceWith(`com.example.receiver${index}`, `draft${index}`)
  } finally {
    await act(async () => root.unmount())
    host.remove()
  }
})
