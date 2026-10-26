import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import test from 'node:test'
import { JSDOM } from 'jsdom'

const source = new URL('../src/ui.js', import.meta.url)
async function components() {
  assert.ok(existsSync(source), 'reusable UI source must exist')
  return import(source.href)
}
function setup(t) {
  const dom = new JSDOM('<button id="outside">Outside</button><nav id="a"></nav><nav id="b"></nav><p id="empty">Choose</p><div id="content"><input value="draft"></div><span id="label"></span>')
  t.after(() => dom.window.close())
  return { document: dom.window.document, window: dom.window }
}
const items = ['a', 'b', 'c'].map((id) => ({ id, title: id }))
test('independent lists preserve keyed nodes and focus on reorder, removal and external focus', async (t) => {
  const { createList } = await components()
  const { document } = setup(t)
  const a = document.querySelector('#a'), b = document.querySelector('#b')
  const calls = []
  const one = createList(a, { onActivate: (id) => calls.push('one:' + id) })
  const two = createList(b, { onActivate: (id) => calls.push('two:' + id) })
  one.update({ items, selectedId: 'a' }); two.update({ items, selectedId: 'b' })
  assert.equal(document.activeElement, document.body)
  one.move(1); one.move(1)
  const focused = document.activeElement
  assert.equal(focused.textContent, 'b')
  one.update({ items: [items[2], items[1], items[0]], selectedId: 'a' })
  assert.equal(document.activeElement, focused)
  assert.equal(a.children[1], focused)
  assert.equal(focused.getAttribute('aria-current'), 'false')
  one.update({ items: [items[2], items[0]], selectedId: 'a' })
  assert.equal(document.activeElement.textContent, 'a')
  document.querySelector('#outside').focus()
  one.update({ items: [items[2]], selectedId: null })
  assert.equal(document.activeElement.id, 'outside')
  two.move(-1); document.activeElement.click()
  assert.deepEqual(calls, ['two:c'])
  one.dispose(); a.querySelector('button').click(); one.move(1)
  assert.deepEqual(calls, ['two:c'])
  two.move(-1); document.activeElement.click()
  assert.deepEqual(calls, ['two:c', 'two:b'])
})
test('list handles plain arrows only, native activation, disabled, empty and literal text', async (t) => {
  const { createList } = await components()
  const { document, window } = setup(t)
  const a = document.querySelector('#a'), calls = []
  const list = createList(a, { onActivate: (id) => calls.push(id) })
  list.update({ items: [{ id: 'a', title: '<img src=x onerror=bad()>' }, items[1]], selectedId: 'a' })
  assert.equal(a.querySelector('img'), null)
  list.move(-1)
  assert.equal(document.activeElement.textContent, 'b')
  const key = (options) => document.activeElement.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true, ...options }))
  assert.equal(key({ ctrlKey: true }), true)
  assert.equal(key({ isComposing: true }), true)
  assert.equal(document.activeElement.textContent, 'b')
  assert.equal(key({}), false)
  assert.match(document.activeElement.textContent, /<img/)
  // JSDOM does not synthesize native key activation. Verify the module leaves
  // Enter/Space to the button; the following click exercises that shared path.
  assert.equal(document.activeElement.type, 'button')
  for (const key of ['Enter', ' ']) {
    const event = new window.KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
    document.activeElement.dispatchEvent(event)
    assert.equal(event.defaultPrevented, false)
  }
  document.activeElement.click()
  assert.deepEqual(calls, ['a'])
  list.update({ items, disabled: true })
  a.querySelector('button').dispatchEvent(new window.MouseEvent('click', { bubbles: true }))
  list.move(1)
  assert.deepEqual(calls, ['a'])
  list.update({ items, disabled: false })
  list.move(1)
  assert.equal(document.activeElement.textContent, 'b')
  list.move(50); assert.equal(document.activeElement.textContent, 'c')
  list.move(1); assert.equal(document.activeElement.textContent, 'c')
  list.update({ items: [], emptyText: '<b>none</b>' })
  assert.equal(a.querySelector('button'), null)
  assert.equal(a.querySelector('b'), null)
  assert.equal(a.textContent, '<b>none</b>')
  list.move(1)
  assert.equal(document.activeElement, document.body)
})
test('detail preserves input and focus while busy; actions and dispose stay instance-local', async (t) => {
  const { createDetail, createActions } = await components()
  const { document, window } = setup(t)
  const empty = document.querySelector('#empty'), content = document.querySelector('#content'), label = document.querySelector('#label')
  const detail = createDetail({ empty, content, label })
  const input = content.firstElementChild
  input.focus(); input.value = 'new draft'
  detail.update({ hasContent: true, busy: true, text: '<b>busy</b>' })
  assert.equal(content.hidden, false); assert.equal(empty.hidden, true)
  assert.equal(content.getAttribute('aria-busy'), 'true')
  assert.equal(document.activeElement, input); assert.equal(input.value, 'new draft')
  assert.equal(label.textContent, '<b>busy</b>'); assert.equal(label.children.length, 0)
  detail.update({ hasContent: false, busy: false, text: 'Choose' })
  assert.equal(empty.hidden, false); assert.equal(content.hidden, true)
  detail.dispose(); detail.update({ hasContent: true, text: 'ignored' })
  assert.equal(content.hidden, true)
  const first = document.createElement('button'), second = document.createElement('button'), calls = []
  const one = createActions({ save: { button: first, onInvoke: () => calls.push(1) } })
  const two = createActions({ save: { button: second, onInvoke: () => calls.push(2) } })
  one.update({ save: { disabled: true } })
  first.dispatchEvent(new window.MouseEvent('click')); second.click()
  assert.deepEqual(calls, [2])
  one.update({ save: { disabled: false } }); first.click()
  one.dispose(); first.click(); second.click()
  assert.deepEqual(calls, [2, 1, 2])
  two.dispose()
})

function disabledFocusFixture(t, createList) {
  const { document, window } = setup(t)
  let documentFocused = true
  // JSDOM keeps disabled buttons focused. Emulate native focus fixup explicitly,
  // and expose whether this WebView owns focus independently from activeElement.
  document.hasFocus = () => documentFocused
  const container = document.querySelector('#a')
  const list = createList(container, { onActivate() {} })
  list.update({ items })
  list.move(1)
  const row = document.activeElement
  list.update({ items, disabled: true })
  document.body.tabIndex = -1
  document.body.focus()
  assert.equal(document.activeElement, document.body)
  return { document, window, list, row, setDocumentFocus: (value) => { documentFocused = value } }
}
test('temporarily disabled row regains focus after native body fixup and accepts arrows', async (t) => {
  const { createList } = await components()
  const { document, window, list, row } = disabledFocusFixture(t, createList)
  list.update({ items, disabled: true })
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, row)
  row.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
  assert.equal(document.activeElement.textContent, 'b')
})
test('reenabling a list never steals focus from editor or another list', async (t) => {
  const { createList } = await components()
  const { document, list } = disabledFocusFixture(t, createList)
  const input = document.querySelector('input')
  input.focus()
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, input)
  list.move(1)
  list.update({ items, disabled: true })
  const second = createList(document.querySelector('#b'), { onActivate() {} })
  second.update({ items }); second.move(1)
  const otherRow = document.activeElement
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, otherRow)
})
test('reenabling after WebView loses focus cannot reclaim Host input focus or defer a reclaim', async (t) => {
  const { createList } = await components()
  const { document, list, setDocumentFocus } = disabledFocusFixture(t, createList)
  setDocumentFocus(false)
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, document.body)
  setDocumentFocus(true)
  list.update({ items })
  assert.equal(document.activeElement, document.body)
})
test('removed rows and disposed instances abandon pending disabled focus', async (t) => {
  const { createList } = await components()
  const { document, list } = disabledFocusFixture(t, createList)
  list.update({ items: items.slice(1), disabled: true })
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, document.body)
  list.move(1); list.update({ items, disabled: true })
  document.body.focus()
  list.dispose()
  list.update({ items, disabled: false })
  assert.equal(document.activeElement, document.body)
})
