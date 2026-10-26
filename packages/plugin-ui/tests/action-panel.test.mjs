import assert from 'node:assert/strict'
import test from 'node:test'
import { JSDOM } from 'jsdom'
import * as ui from '../src/ui.js'
function setup(t) {
  assert.equal(typeof ui.createActionPanel, 'function', 'createActionPanel must be exported')
  const dom = new JSDOM('<input id="before"><button id="trigger">Actions</button><dialog id="dialog" aria-labelledby="heading"><h2 id="heading">Actions</h2><button id="close">Close</button><div id="list"></div><button id="cancel">Cancel</button></dialog><dialog id="other"></dialog>')
  t.after(() => dom.window.close())
  const document = dom.window.document, node = (id) => document.getElementById(id), dialog = node('dialog'), calls = []
  const closes = []
  dialog.showModal = () => { dialog.open = true }
  dialog.close = () => { dialog.open = false; closes.push(() => dialog.dispatchEvent(new dom.window.Event('close'))) }
  const panel = ui.createActionPanel({ dialog, trigger: node('trigger'), list: node('list'), closeButton: node('close'), cancelButton: node('cancel'), actions: ['one','two','constructor'].map((id) => ({ id, label: id === 'one' ? '<b>One</b>' : id, onInvoke: () => { assert.equal(dialog.open, false); calls.push(id); node('other').open = true } })) })
  const rows = [...node('list').children]
  const key = (key, more = {}) => { const event = new dom.window.KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...more }); document.activeElement.dispatchEvent(event); return event.defaultPrevented }
  return { panel, node, document, window: dom.window, dialog, rows, calls, closes, key }
}
test('ActionPanel uses latest availability, one invocation and close-before-business without late close effects', (t) => {
  const { panel, node, dialog, rows, calls, closes, window } = setup(t)
  node('before').focus(); panel.open(); panel.open()
  assert.equal(dialog.open, true); assert.equal(rows[0].type, 'button'); assert.equal(rows[0].children.length, 0)
  panel.update({ one: { disabled: true }, two: { hidden: true } })
  rows[0].dispatchEvent(new window.MouseEvent('click', { bubbles: true }))
  rows[1].dispatchEvent(new window.MouseEvent('click', { bubbles: true }))
  assert.deepEqual(calls, [])
  rows[2].click(); rows[2].click()
  assert.deepEqual(calls, ['constructor'])
  closes.shift()(); assert.equal(node('other').open, true)
  panel.open(); assert.equal(dialog.open, false, 'another modal blocks opening')
})
test('ActionPanel navigation, state focus, native activation and cancellation stay inside its modal', (t) => {
  const { panel, node, document, window, rows, key, dialog, calls } = setup(t)
  node('before').focus(); panel.open(); assert.equal(document.activeElement, rows[0])
  assert.equal(key('ArrowDown', { ctrlKey: true }), false); assert.equal(key('ArrowDown', { isComposing: true }), false)
  key('End'); assert.equal(document.activeElement, rows[2]); key('ArrowDown'); assert.equal(document.activeElement, rows[2])
  key('Home'); key('ArrowDown'); assert.equal(document.activeElement, rows[1])
  panel.update(new Map([['two', { hidden: true }]])); assert.ok(dialog.contains(document.activeElement)); assert.notEqual(document.activeElement, rows[1])
  assert.equal(key('Enter'), false); assert.equal(key(' '), false)
  panel.update({ one: { disabled: true }, two: { hidden: true }, constructor: { disabled: true } })
  assert.equal(document.activeElement, node('cancel'))
  node('cancel').click(); assert.equal(document.activeElement, node('before'))
  panel.open(); node('close').click(); assert.equal(dialog.open, false)
  panel.open(); const cancel = new window.Event('cancel', { cancelable: true }); dialog.dispatchEvent(cancel)
  assert.equal(cancel.defaultPrevented, true); assert.equal(dialog.open, false); assert.deepEqual(calls, [])
})
test('ActionPanel ignores inherited state and abandons restoration after document focus loss or dispose', (t) => {
  const { panel, node, document, dialog, rows, closes } = setup(t)
  panel.update(Object.create({ constructor: { disabled: true } }))
  assert.equal(rows[2].disabled, false)
  node('before').focus(); panel.open(); document.hasFocus = () => false
  panel.close(); assert.notEqual(document.activeElement, node('before'))
  document.hasFocus = () => true
  panel.open(); panel.close({ restoreFocus: false }); node('trigger').focus(); panel.open()
  closes.shift()(); assert.equal(dialog.open, true); assert.equal(document.activeElement, rows[0])
  panel.dispose(); rows[0].click(); node('trigger').click(); panel.open()
  assert.equal(dialog.open, false); assert.notEqual(document.activeElement, node('before'))
})

test('two ActionPanel instances in one document keep state and disposal independent', (t) => {
  const { panel, node, document, rows, calls } = setup(t)
  const trigger = document.createElement('button'), list = document.createElement('div'), cancelButton = document.createElement('button'), closeButton = document.createElement('button')
  document.body.append(trigger)
  const dialog = node('other'); dialog.append(list, closeButton, cancelButton)
  dialog.showModal = () => { dialog.open = true }
  dialog.close = () => { dialog.open = false }
  const second = ui.createActionPanel({ dialog, trigger, list, cancelButton, closeButton, actions: [{ id: 'one', label: 'Second', onInvoke: () => calls.push('second') }] })
  panel.update({ one: { disabled: true } })
  assert.equal(list.firstElementChild.disabled, false)
  second.open(); panel.open(); assert.equal(node('dialog').open, false)
  panel.dispose(); assert.equal(dialog.open, true)
  list.firstElementChild.click(); assert.deepEqual(calls, ['second'])
  rows[0].click(); assert.deepEqual(calls, ['second'])
  second.dispose()
})
