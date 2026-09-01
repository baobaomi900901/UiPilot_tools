import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const logicUrl = new URL('../package/dist/clipboard-history-logic.js', import.meta.url)
const manifestUrl = new URL('../package/plugin.json', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)

async function loadLogic() {
  return import(`${logicUrl.href}?test=${Date.now()}`)
}

test('compares canonical u64 revisions without converting them to numbers', async () => {
  const { compareRevisions } = await loadLogic()

  assert.equal(compareRevisions('9', '10'), -1)
  assert.equal(compareRevisions('10', '9'), 1)
  assert.equal(compareRevisions('18446744073709551614', '18446744073709551615'), -1)
  assert.equal(compareRevisions('42', '42'), 0)
})

const entries = Object.freeze([
  Object.freeze({ id: 'text-2', kind: 'text' }),
  Object.freeze({ id: 'image-1', kind: 'image' }),
  Object.freeze({ id: 'files-1', kind: 'files' }),
  Object.freeze({ id: 'text-1', kind: 'text' }),
])

test('accepts only snapshots newer than the current canonical revision', async () => {
  const { shouldApplySnapshot } = await loadLogic()

  assert.equal(shouldApplySnapshot(null, '0'), true)
  assert.equal(shouldApplySnapshot('9', '10'), true)
  assert.equal(shouldApplySnapshot('10', '10'), false)
  assert.equal(shouldApplySnapshot('10', '9'), false)
})

test('filters entries by the approved tabs without changing Host order', async () => {
  const { filterEntries } = await loadLogic()

  assert.deepEqual(filterEntries(entries, 'all').map(({ id }) => id), ['text-2', 'image-1', 'files-1', 'text-1'])
  assert.deepEqual(filterEntries(entries, 'image').map(({ id }) => id), ['image-1'])
  assert.deepEqual(filterEntries(entries, 'files').map(({ id }) => id), ['files-1'])
  assert.deepEqual(filterEntries(entries, 'text').map(({ id }) => id), ['text-2', 'text-1'])
})

test('cycles filters forward and backward with wrapping', async () => {
  const { cycleFilter } = await loadLogic()

  assert.equal(cycleFilter('all', 1), 'image')
  assert.equal(cycleFilter('text', 1), 'all')
  assert.equal(cycleFilter('all', -1), 'text')
  assert.equal(cycleFilter('files', -1), 'image')
})

test('moves selection within visible entries and clamps at the boundaries', async () => {
  const { moveSelection } = await loadLogic()

  assert.equal(moveSelection(entries, null, 1), 'text-2')
  assert.equal(moveSelection(entries, 'text-2', -1), 'text-2')
  assert.equal(moveSelection(entries, 'text-2', 1), 'image-1')
  assert.equal(moveSelection(entries, 'text-1', 1), 'text-1')
  assert.equal(moveSelection([], 'text-1', 1), null)
})

test('keeps a visible selection and otherwise selects the newest entry', async () => {
  const { reconcileSelection } = await loadLogic()

  assert.equal(reconcileSelection(entries, 'files-1'), 'files-1')
  assert.equal(reconcileSelection(entries, 'missing'), 'text-2')
  assert.equal(reconcileSelection([], 'missing'), null)
})

test('manifest declares the Windows clipboard history Panel contract', async () => {
  const manifest = JSON.parse(await readFile(manifestUrl, 'utf8'))

  assert.equal(manifest.pluginId, 'com.uipilot.clipboard-history')
  assert.equal(manifest.version, '1.0.9')
  assert.match(manifest.description, /最近 50 条/)
  assert.equal(manifest.minimumHostVersion, '0.3.4')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.panel, {
    entry: 'dist/panel.html',
    hostKeys: ['ArrowDown', 'ArrowUp', 'Tab', 'Shift+Tab', 'Enter'],
    hostKeyFocus: 'host',
  })
  assert.deepEqual(manifest.permissions, [
    'ui.panel',
    'clipboard.history.read',
    'clipboard.history.paste',
  ])
  assert.equal('window' in manifest, false)
})

test('runtime preserves request ownership and returns an empty Panel payload', async () => {
  const runtime = await import(`${runtimeUrl.href}?test=${Date.now()}`)
  const invocation = Object.freeze({
    apiVersion: 1,
    requestId: 'clipboard-history-request-1',
    input: '',
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-30T14:00:00+08:00',
    }),
  })

  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'clipboard-history-request-1',
    data: {},
  })
})
