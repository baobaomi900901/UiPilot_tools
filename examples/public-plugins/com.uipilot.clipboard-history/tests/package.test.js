import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const exampleRoot = new URL('../', import.meta.url)
const packageRoot = new URL('../package/', import.meta.url)

test('strict package root contains only the public clipboard history Panel assets', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual((await readdir(new URL('dist/', packageRoot))).sort(), [
    'clipboard-history-logic.js',
    'panel.css',
    'panel.html',
    'panel.js',
    'runtime.js',
  ])
})

test('preview covers text, image, available files, unavailable files, and both themes', async () => {
  const [html, source] = await Promise.all([
    readFile(new URL('preview.html', exampleRoot), 'utf8'),
    readFile(new URL('preview.js', exampleRoot), 'utf8'),
  ])

  assert.match(html, /Clipboard History Preview/)
  for (const required of [
    "kind: 'text'",
    "kind: 'image'",
    "kind: 'files'",
    'available: false',
    "get('theme') === 'light'",
    'clipboardHistory',
    "key: 'Enter'",
  ]) {
    assert.match(source, new RegExp(required.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
  }
})

test('README documents permissions, privacy boundary, preview, verification, and manual acceptance', async () => {
  const readme = await readFile(new URL('README.md', exampleRoot), 'utf8')

  for (const required of [
    'clipboard.history.read',
    'clipboard.history.paste',
    '不会获得完整文本、原图或完整文件路径',
    'preview.html',
    'node --test --experimental-test-isolation=none',
    'uipilot-plugin validate',
    '2026-08-30-clipboard-history-host-manual-acceptance.md',
  ]) {
    assert.match(readme, new RegExp(required.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
  }
})
