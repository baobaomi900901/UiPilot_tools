import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { JSDOM } from 'jsdom'

test('window dialog close buttons cancel without saving, deleting or discarding an editor draft', async (t) => {
  const html = await readFile(new URL('../package/dist/window.html', import.meta.url), 'utf8')
  const source = await readFile(new URL('../package/dist/window.js', import.meta.url), 'utf8')
  const dom = new JSDOM(html, { runScripts: 'outside-only', pretendToBeVisual: true })
  t.after(() => dom.window.close())
  const { window } = dom
  const node = (id) => window.document.querySelector('#' + id)
  const flush = () => new Promise((resolve) => setImmediate(resolve))
  const notes = [
    { id: '1', title: 'First', content: 'one', createdAt: '2026-08-27T10:00:00Z' },
    { id: '2', title: 'Second', content: 'two', createdAt: '2026-08-27T09:00:00Z' },
  ]
  let stored = structuredClone(notes)
  let update
  window.HTMLDialogElement.prototype.showModal = function () { this.open = true }
  window.HTMLDialogElement.prototype.close = function () { this.open = false }
  window.uipilotPluginWindow = {
    onUpdate(handler) { update = handler },
    storage: {
      async get() { return structuredClone(stored) },
      async set(_key, value) { stored = structuredClone(value) },
    },
  }
  window.eval(source)
  await update({ theme: 'dark' })
  const close = async (id) => {
    const button = node(id + '-close-btn')
    assert.ok(button)
    assert.equal(button.type, 'button')
    assert.equal(button.getAttribute('aria-label'), '关闭')
    button.click()
    await flush()
    assert.equal(node(id + '-dialog').open, false)
  }
  window.document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }))
  assert.equal(node('new-dialog').open, true)
  node('new-title-input').value = 'Cancelled new note'
  await close('new')
  assert.equal(node('new-title-input').value, '')
  window.document.querySelector('[data-note-id="1"].note-delete').click()
  assert.equal(node('delete-dialog').open, true)
  await close('delete')
  window.document.querySelector('[data-note-id="1"].note-select').click()
  await flush()
  node('editor-content').value = 'Keep this draft'
  node('editor-content').dispatchEvent(new window.Event('input', { bubbles: true }))
  window.document.querySelector('[data-note-id="2"].note-select').click()
  await flush()
  assert.equal(node('unsaved-dialog').open, true)
  await close('unsaved')
  assert.equal(node('editor-content').value, 'Keep this draft')
  assert.deepEqual(stored, notes)
  node('editor-content').value = 'one'
  window.document.querySelector('[data-note-id="2"].note-select').click()
  await flush()
  assert.equal(node('editor-content').value, 'two')
})
