import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { snapshotGraph, bundleSnapshot } from './bundle.mjs'
async function fixture(files, body) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'uipilot-esm-builder-')))
  try {
    for (const [name, source] of Object.entries(files)) {
      await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true })
      await fs.writeFile(path.join(root, name), source)
    }
    await body(root)
  } finally { await fs.rm(root, { recursive: true }) }
}
test('AST rejects dynamic imports, including computed and dead branches, and import.meta', async () => {
  for (const source of ["import('./child.js')", "const x='./child.js'; import(x)", "if(false)import('./child.js')", 'export const url=import.meta.url']) {
    await fixture({ 'entry.js': source, 'child.js': 'export const value=1' }, async root => {
      await assert.rejects(snapshotGraph(root, 'entry.js'))
    })
  }
})
test('relative graph bundles from the immutable snapshot, retaining top-level side effects', async () => {
  await fixture({ 'entry.js': "import { x } from './child.js'; export const value=x;", 'child.js': 'globalThis.TOP_LEVEL_PROBE=1; export const x=2;' }, async root => {
    const graph = await snapshotGraph(root, 'entry.js')
    assert.equal(graph.modules.size, 2)
    await fs.writeFile(path.join(root, 'child.js'), 'invalid changed disk')
    const result = await bundleSnapshot(graph)
    assert.match(result.code, /TOP_LEVEL_PROBE/)
    assert.equal(result.inputs.length, 2)
  })
})
test('rejects package escape, bare imports, URLs, symlinks and unsupported assets', async () => {
  for (const specifier of ['../outside.js', 'node:fs', 'library', 'https://example.invalid/x.js', './asset.json']) {
    await fixture({ 'entry.js': `import '${specifier}'` }, async root => {
      await assert.rejects(snapshotGraph(root, 'entry.js'))
    })
  }
  await fixture({ 'entry.js': "import './link.js'", 'actual.js': 'export const x=1' }, async root => {
    await fs.symlink('actual.js', path.join(root, 'link.js'))
    await assert.rejects(snapshotGraph(root, 'entry.js'))
  })
})
test('enforces file count, per-file bytes and total graph budget', async () => {
  await fixture({ 'entry.js': ' '.repeat(128 * 1024 + 1) }, async root => assert.rejects(snapshotGraph(root, 'entry.js')))
  for (const [count, padding] of [[33, 0], [10, 120 * 1024]]) {
    const files = {}
    for (let i = 0; i < count; i++) files[`${i}.js`] = `${i + 1 < count ? `import './${i + 1}.js';` : ''}\n${' '.repeat(padding)}`
    await fixture(files, async root => assert.rejects(snapshotGraph(root, '0.js')))
  }
})
test('never evaluates top-level code during graph loading or bundling', async () => {
  await fixture({ 'entry.js': 'while(true){}; export function onCommand(){}' }, async root => {
    const result = await bundleSnapshot(await snapshotGraph(root, 'entry.js'))
    assert.match(result.code, /while \(true\)/)
  })
})

test('keeps PURE-annotated top-level calls and enforces strict ESM evaluation', async () => {
  await fixture({ 'entry.js': '/* @__PURE__ */ (() => { globalThis.PURE_EFFECT = 1 })(); export const value=2' }, async root => {
    const result = await bundleSnapshot(await snapshotGraph(root, 'entry.js'))
    assert.match(result.code, /^"use strict";/)
    assert.match(result.code, /PURE_EFFECT/)
    assert.equal(result.compiler, '0.28.2')
    assert.equal(result.parser, '1.1.5')
  })
})
