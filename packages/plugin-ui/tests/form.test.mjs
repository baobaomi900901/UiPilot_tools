import assert from 'node:assert/strict'
import test from 'node:test'
import { JSDOM } from 'jsdom'
import * as ui from '../src/ui.js'
function setup(t) {
  assert.equal(typeof ui.createForm, 'function', 'createForm must be exported')
  const dom = new JSDOM('<input id="a" value="original" aria-describedby="hint"><p id="ae"></p><textarea id="b">body</textarea><p id="be"></p><input id="check" type="checkbox" checked><p id="ce"></p><select id="select"><option value="v" selected>V</option></select><p id="se"></p>')
  t.after(() => dom.window.close())
  const document = dom.window.document, node = (id) => document.getElementById(id)
  const fields = [ ['a','ae'], ['b','be'], ['check','ce'], ['select','se'] ].map(([id, error]) => ({ id, control: node(id), error: node(error), validate: () => 'Invalid ' + id }))
  return { document, window: dom.window, node, fields }
}
test('Form preserves native values, isolates instances and validates only the requested fields', (t) => {
  const { document, node, fields } = setup(t)
  const first = ui.createForm({ fields: fields.slice(0, 2) }), second = ui.createForm({ fields: fields.slice(2) })
  node('b').focus()
  assert.equal(first.validate(['b']), false)
  assert.equal(node('ae').textContent, '')
  assert.equal(node('b').getAttribute('aria-invalid'), 'true')
  assert.equal(document.activeElement, node('b'))
  assert.equal(second.validate(), false)
  assert.equal(node('check').checked, true)
  assert.equal(node('select').value, 'v')
  assert.equal(node('a').value, 'original'); assert.equal(node('b').value, 'body')
  first.clear(); assert.equal(node('ce').textContent, 'Invalid check')
  document.hasFocus = () => false
  node('b').focus(); first.validate(['a'])
  assert.equal(document.activeElement, node('b'), 'validation cannot reclaim focus from Host')
})
test('Form owns only its errors and description token, keeps stable fields and releases removed listeners', (t) => {
  const { node, fields, window } = setup(t)
  const form = ui.createForm({ fields })
  form.setErrors(new Map([['a', '<b>bad</b>'], ['unknown', 'ignored']]))
  assert.equal(node('ae').innerHTML, '&lt;b&gt;bad&lt;/b&gt;')
  assert.equal(node('a').getAttribute('aria-describedby'), 'hint ae')
  node('a').setAttribute('aria-describedby', 'hint ae caller')
  form.setFields(fields); assert.equal(node('ae').textContent, '<b>bad</b>')
  node('a').dispatchEvent(new window.Event('input'))
  assert.equal(node('a').getAttribute('aria-describedby'), 'hint caller')
  form.setErrors(new Map([['a', 'again']]))
  form.setFields(fields.slice(1))
  assert.equal(node('a').getAttribute('aria-invalid'), null)
  node('ae').textContent = 'caller-owned now'
  node('a').dispatchEvent(new window.Event('change'))
  assert.equal(node('ae').textContent, 'caller-owned now')
  form.setErrors(new Map([['b', 'bad']]))
  form.dispose(); assert.equal(node('be').textContent, '')
  node('be').textContent = 'after dispose'
  form.validate(); form.setErrors(new Map([['b','ignored']])); form.clear(); form.setFields(fields)
  node('b').dispatchEvent(new window.Event('input'))
  assert.equal(node('be').textContent, 'after dispose')
})

test('Form restores caller invalid state and keeps a preexisting error description token', (t) => {
  const { node, fields } = setup(t)
  node('a').setAttribute('aria-describedby', 'hint ae')
  node('a').setAttribute('aria-invalid', 'grammar')
  const form = ui.createForm({ fields })
  form.setErrors(new Map([['a', 'component error']]))
  form.clear(['a'])
  assert.equal(node('a').getAttribute('aria-describedby'), 'hint ae')
  assert.equal(node('a').getAttribute('aria-invalid'), 'grammar')
  const replacement = node('b').cloneNode()
  replacement.id = 'replacement'
  node('b').after(replacement)
  form.setErrors(new Map([['b', 'old error']]))
  form.setFields([{ ...fields[1], control: replacement }])
  assert.equal(node('b').getAttribute('aria-invalid'), null)
  form.setErrors(new Map([['b', 'new error']]))
  assert.equal(replacement.getAttribute('aria-invalid'), 'true')
  form.dispose()
})
