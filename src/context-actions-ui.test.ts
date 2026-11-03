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
