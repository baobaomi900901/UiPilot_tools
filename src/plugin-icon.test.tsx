// @vitest-environment jsdom

import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, describe, expect, it } from 'vitest'

import { PluginIcon } from './plugin-icon'
import { safePublicPluginIconUrl } from './plugin-icon-url'

const installed = 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png'
const windowsInstalled = 'http://uipilot-public-plugin.localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png'
const prepared = 'uipilot-public-plugin://localhost/__uipilot_icon/prepared/public-prepare-0000000000000001-0000000000000002/icon.png'

let host: HTMLDivElement | undefined
let root: ReturnType<typeof createRoot> | undefined

afterEach(() => {
  if (root) act(() => root?.unmount())
  host?.remove()
  root = undefined
  host = undefined
})

describe('public plugin icon', () => {
  it('accepts only exact host-issued installed and prepared URLs', () => {
    expect(safePublicPluginIconUrl(installed)).toBe(installed)
    expect(safePublicPluginIconUrl(windowsInstalled)).toBe(windowsInstalled)
    expect(safePublicPluginIconUrl(prepared)).toBe(prepared)
    for (const invalid of [
      'https://example.com/icon.png',
      `${installed}?cache=bust`,
      installed.replace('/1/', '/01/'),
      installed.replace('icon.png', 'Icon.png'),
      prepared.replace('0000000000000002', '000000000000000G'),
    ]) expect(safePublicPluginIconUrl(invalid)).toBeUndefined()
  })

  it('uses a stable fallback when loading fails', async () => {
    host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    await act(async () => root?.render(<PluginIcon iconUrl={installed} size={28} />))
    const image = host.querySelector<HTMLImageElement>('.plugin-icon-image')!
    const fallback = host.querySelector<HTMLElement>('.plugin-icon-fallback')!
    expect(image.hidden).toBe(false)
    expect(fallback.hidden).toBe(true)
    await act(async () => image.dispatchEvent(new Event('error')))
    expect(image.hidden).toBe(true)
    expect(fallback.hidden).toBe(false)
  })
})
