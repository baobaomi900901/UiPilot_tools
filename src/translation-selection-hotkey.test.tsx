// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { beforeEach, expect, it, vi } from 'vitest'
import { TranslationSelectionHotkey } from './translation-selection-hotkey'
import { installationPermissionGrants } from './host-credential-permissions'
import type { LauncherClient } from './protocol'
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }))
})
it('does not grant selected text access during installation', () => {
  expect(installationPermissionGrants(['selection.read', 'network.https', 'credentials.youdaoTranslation'])).toEqual(['network.https'])
})
it('refreshes real settings after failed clear without retrying the write', async () => {
  const host = document.createElement('div'); document.body.append(host); const root = createRoot(host)
  const getTranslationSelectionSettings = vi.fn(async () => ({ hotkey: 'Alt+A', status: 'suspended' as const }))
  const setTranslationSelectionHotkey = vi.fn(async () => { throw new Error('conflict') })
  try {
    await act(async () => root.render(<TranslationSelectionHotkey disabled={false} client={{ getTranslationSelectionSettings, setTranslationSelectionHotkey } as unknown as LauncherClient} />))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="清除划词翻译快捷键"]')!.click())
    expect(setTranslationSelectionHotkey).toHaveBeenCalledExactlyOnceWith({ hotkey: null })
    expect(getTranslationSelectionSettings).toHaveBeenCalledTimes(2)
    expect(host.querySelector<HTMLInputElement>('[aria-label="划词翻译快捷键"]')!.value).toContain('A')
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('无法保存')
  } finally { await act(async () => root.unmount()); host.remove() }
})

it('keeps speech opt-in independent of installation consent', () => { expect(installationPermissionGrants(['speech.speak', 'clipboard.write'])).toEqual(['clipboard.write']) })
