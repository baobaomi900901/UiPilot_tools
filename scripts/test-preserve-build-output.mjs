import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, statSync, utimesSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { snapshotOutput, restoreUnchangedOutput } from './preserve-build-output.mjs'

test('same bytes retain timestamps; changed/new files keep fresh timestamps', () => {
  const root = mkdtempSync(join(tmpdir(), 'uipilot-output-'))
  try {
    mkdirSync(join(root, 'assets'))
    for (const name of ['index.html', 'assets/app.js', 'removed.css']) {
      writeFileSync(join(root, name), 'old')
      utimesSync(join(root, name), 1000000000, 1000000000)
    }
    const before = snapshotOutput(root)
    // Model Vite's normal emptyOutDir and write, including removed assets.
    rmSync(join(root, 'assets'), { recursive: true })
    rmSync(join(root, 'removed.css'))
    mkdirSync(join(root, 'assets'))
    writeFileSync(join(root, 'index.html'), 'old')
    writeFileSync(join(root, 'assets/app.js'), 'new')
    writeFileSync(join(root, 'assets/new.js'), 'new')
    assert.equal(restoreUnchangedOutput(root, before), 1)
    assert.equal(statSync(join(root, 'index.html')).mtimeMs, 1000000000000)
    assert.ok(statSync(join(root, 'assets/app.js')).mtimeMs > 1000000000000)
    assert.ok(statSync(join(root, 'assets/new.js')).mtimeMs > 1000000000000)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('first build has no timestamps to restore', () => {
  const root = join(tmpdir(), 'uipilot-missing-output-' + crypto.randomUUID())
  assert.equal(snapshotOutput(root).size, 0)
})
