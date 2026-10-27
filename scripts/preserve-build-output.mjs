// Cargo tracks embedded frontend files by mtime. Vite still builds and removes
// stale assets normally; only byte-identical files regain their old timestamp.
import { lstatSync, readdirSync, readFileSync, utimesSync } from 'node:fs'
import { join, resolve } from 'node:path'

export function snapshotOutput(root) {
  const files = new Map()
  function visit(directory) {
    let info
    try { info = lstatSync(directory) } catch (error) {
      if (error.code === 'ENOENT') return
      throw error
    }
    if (!info.isDirectory() || info.isSymbolicLink()) return
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name)
      if (entry.isDirectory()) visit(path)
      else if (entry.isFile()) {
        const stat = lstatSync(path)
        files.set(path, { content: readFileSync(path), atime: stat.atime, mtime: stat.mtime })
      }
    }
  }
  visit(root)
  return files
}

export function restoreUnchangedOutput(root, before) {
  let retained = 0
  // Walk the current tree too, so removed files and linked directories are ignored.
  for (const [path, current] of snapshotOutput(root)) {
    const old = before.get(path)
    if (old && old.content.equals(current.content)) {
      utimesSync(path, old.atime, old.mtime)
      retained++
    }
  }
  return retained
}

export function preserveBuildOutput() {
  let root
  let before
  return {
    name: 'uipilot-preserve-identical-output',
    apply: 'build',
    configResolved(config) { root = resolve(config.root, config.build.outDir) },
    buildStart() { before = snapshotOutput(root) },
    writeBundle() {
      const count = restoreUnchangedOutput(root, before)
      console.log(`UIPILOT_UNCHANGED_ASSETS ${count}`)
    },
  }
}
