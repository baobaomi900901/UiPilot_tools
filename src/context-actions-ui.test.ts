// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { createContextActionsMenu } from './context-actions-ui.js'

describe('context actions menu', () => {
  it('shows only grouped actions and shortcuts and restores the original selection', () => {
    document.body.innerHTML = '<textarea>sample text</textarea>'
    const input = document.querySelector('textarea')!
    input.focus(); input.setSelectionRange(2, 6)
    const menu = createContextActionsMenu(document)
    menu.open({ context: [{ id: 'save', title: '保存', shortcut: 'Ctrl+S', run: vi.fn() }], global: [{ id: 'new', title: '新建', shortcut: 'Ctrl+N', run: vi.fn() }] })
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.textContent).toContain('当前对象')
    expect(dialog.textContent).toContain('通用操作')
    expect(dialog.textContent).not.toMatch(/点击执行|↑↓选择/)
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(menu.isOpen).toBe(false)
    expect(document.activeElement).toBe(input)
    expect([input.selectionStart, input.selectionEnd]).toEqual([2, 6])
  })

  it('skips disabled actions and executes once without leaking menu keys', async () => {
    document.body.innerHTML = '<input>'
    const run = vi.fn()
    const pluginKey = vi.fn()
    const menu = createContextActionsMenu(document)
    menu.open({ context: [{ id: 'blocked', title: '保存', disabledReason: '没有选中笔记', run }, { id: 'copy', title: '复制', run }] })
    document.addEventListener('keydown', pluginKey)
    const buttons = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')]
    buttons[0]!.click()
    expect(run).not.toHaveBeenCalled()
    buttons[1]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    buttons[1]!.click()
    expect(run).toHaveBeenCalledOnce()
    expect(pluginKey).not.toHaveBeenCalled()
    document.removeEventListener('keydown', pluginKey)
  })

  it('invalidates detached actions and rejects malformed groups before opening', () => {
    const menu = createContextActionsMenu(document)
    const run = vi.fn()
    menu.open({ context: [{ id: 'copy', title: '复制', run }] })
    const oldButton = document.querySelector<HTMLButtonElement>('.uipilot-context-action')!
    menu.close(false)
    oldButton.click()
    expect(run).not.toHaveBeenCalled()
    expect(() => menu.open({ context: [{ id: 'bad', title: 'x'.repeat(81), run }] })).toThrow()
    expect(menu.isOpen).toBe(false)
  })
  it('uses a virtual scrollbar for overflow, track clicks and dragging, then removes its listeners', () => {
    const menu = createContextActionsMenu(document)
    menu.open({ context: Array.from({ length: 12 }, (_, index) => ({ id: `action-${index}`, title: `操作 ${index}`, run: vi.fn() })) })
    const viewport = document.querySelector<HTMLElement>('.uipilot-context-actions-viewport')!
    const track = document.querySelector<HTMLElement>('.uipilot-context-actions-track')!
    const thumb = document.querySelector<HTMLElement>('.uipilot-context-actions-thumb')!
    expect(viewport).not.toBeNull()
    expect(track.hidden).toBe(true)
    Object.defineProperties(viewport, { clientHeight: { configurable: true, value: 100 }, scrollHeight: { configurable: true, value: 500 } })
    Object.defineProperty(track, 'clientHeight', { value: 84 })
    vi.spyOn(track, 'getBoundingClientRect').mockReturnValue({ top: 10 } as DOMRect)
    window.dispatchEvent(new Event('resize'))
    expect(track.hidden).toBe(false)
    expect(thumb.style.height).toBe('24px')
    viewport.scrollTop = 200
    viewport.dispatchEvent(new Event('scroll'))
    expect(thumb.style.transform).toBe('translateY(30px)')
    thumb.dispatchEvent(new MouseEvent('pointerdown', { button: 0, clientY: 40, bubbles: true, cancelable: true }))
    document.dispatchEvent(new MouseEvent('pointermove', { clientY: 70, bubbles: true }))
    expect(viewport.scrollTop).toBe(400)
    document.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    track.dispatchEvent(new MouseEvent('pointerdown', { button: 0, clientY: 10, bubbles: true, cancelable: true }))
    expect(viewport.scrollTop).toBe(0)
    menu.close(false)
    document.dispatchEvent(new MouseEvent('pointermove', { clientY: 90, bubbles: true }))
    expect(viewport.scrollTop).toBe(0)
    expect(document.querySelector('.uipilot-context-actions-track')).toBeNull()
  })

})


it.each(['control', 'meta'] as const)('uses an explicit keyboard selection and the %s menu shortcut', (primaryModifier) => {
  document.body.innerHTML = '<input value="keep selection">'
  const input = document.querySelector('input')!; input.focus(); input.setSelectionRange(1, 4)
  const first = vi.fn(); const second = vi.fn()
  const menu = createContextActionsMenu(document)
  const open = () => menu.open({ context: [{ id: 'first', title: '第一项', run: first }, { id: 'disabled', title: '不可用', disabledReason: '未选中', run: vi.fn() }, { id: 'second', title: '第二项', run: second }] }, { primaryModifier })
  const key = (key: string, extra: KeyboardEventInit = {}) => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra }))
  open()
  const buttons = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')]
  expect(buttons[0].getAttribute('tabindex')).toBe('0')
  expect(document.activeElement).toBe(buttons[0])
  // A host refocus or native focus refusal must not erase the menu's selected action.
  input.focus(); key('ArrowDown')
  expect(document.activeElement).toBe(buttons[2])
  expect(buttons[2].classList.contains('is-active')).toBe(true)
  key('ArrowUp'); expect(document.activeElement).toBe(buttons[0])
  key('ArrowDown'); key('Enter', { repeat: true }); expect(second).not.toHaveBeenCalled()
  key('Enter'); expect(second).toHaveBeenCalledOnce(); expect(first).not.toHaveBeenCalled()
  expect(document.activeElement).toBe(input)
  open()
  key('k', primaryModifier === 'meta' ? { ctrlKey: true } : { metaKey: true })
  expect(menu.isOpen).toBe(true)
  key('k', primaryModifier === 'meta' ? { metaKey: true } : { ctrlKey: true })
  expect(menu.isOpen).toBe(false)
  expect([input.selectionStart, input.selectionEnd]).toEqual([1, 4])
})

it('retains a usable menu selection when native button focus is refused', () => {
  document.body.innerHTML = '<input>'
  const run = vi.fn(); const menu = createContextActionsMenu(document)
  const nativeFocus = HTMLElement.prototype.focus
  const focus = vi.spyOn(HTMLElement.prototype, 'focus').mockImplementation(function(this: HTMLElement, options) {
    if (!this.matches('.uipilot-context-action')) nativeFocus.call(this, options)
  })
  try {
    menu.open({ context: [{ id: 'first', title: 'First', run: vi.fn() }, { id: 'second', title: 'Second', run }] })
    expect(document.activeElement?.classList.contains('uipilot-context-actions')).toBe(true)
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    expect(document.querySelector('.uipilot-context-action.is-active')?.textContent).toBe('Second')
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    expect(run).toHaveBeenCalledOnce()
  } finally { menu.close(false); focus.mockRestore() }
})
