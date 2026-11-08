import fs from 'node:fs/promises'
import { constants } from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { parseAst } from 'rolldown/parseAst'
import { VERSION as parserVersion } from 'rolldown'
import * as esbuild from 'esbuild'

const MAX_FILES = 32
const MAX_FILE = 128 * 1024
const MAX_TOTAL = 1024 * 1024
const fail = () => { throw new Error('moduleGraphRejected') }
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex')
const within = relative => relative && relative !== '..' && !relative.startsWith('../') && !path.isAbsolute(relative)
function resolve(importer, specifier) {
  if (typeof specifier !== 'string' || !/^\.\.?\//.test(specifier) || /[\\\0?#%:]/.test(specifier)) fail()
  const name = path.posix.normalize(path.posix.join(path.posix.dirname(importer), specifier))
  if (!within(name) || !name.endsWith('.js') || name.split('/').length > 8) fail()
  return name
}
function dependencies(source) {
  let ast
  try { ast = parseAst(source, { sourceType: 'module' }) } catch { fail() }
  const pending = [ast]
  const imports = []
  while (pending.length) {
    const node = pending.pop()
    if (!node || typeof node !== 'object') continue
    if (node.type === 'ImportExpression' || node.type === 'MetaProperty') fail()
    if (['ImportDeclaration', 'ExportNamedDeclaration', 'ExportAllDeclaration'].includes(node.type) && node.source) {
      if (typeof node.source.value !== 'string' || node.attributes?.length) fail()
      imports.push(node.source.value)
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) pending.push(...value)
      else if (value && typeof value === 'object') pending.push(value)
    }
  }
  return imports
}
async function readModule(root, name) {
  const pieces = name.split('/')
  let target = root
  let named
  for (const [index, piece] of pieces.entries()) {
    target = path.join(target, piece)
    const info = await fs.lstat(target, { bigint: true })
    named = info
    if (info.isSymbolicLink() || (index < pieces.length - 1 ? !info.isDirectory() : !info.isFile())) fail()
  }
  if (await fs.realpath(target) !== target) fail()
  const file = await fs.open(target, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const info = await file.stat({ bigint: true })
    if (!info.isFile() || info.size > MAX_FILE || info.ino !== named.ino || info.dev !== named.dev) fail()
    const buffer = Buffer.alloc(MAX_FILE + 1)
    let count = 0
    while (count < buffer.length) {
      const read = await file.read(buffer, count, buffer.length - count, count)
      if (!read.bytesRead) break
      count += read.bytesRead
    }
    const after = await fs.lstat(target, { bigint: true })
    const final = await file.stat({ bigint: true })
    const stable = ['ino', 'dev', 'size', 'mtimeNs', 'ctimeNs']
      .every(key => named[key] === info[key] && info[key] === final[key] && final[key] === after[key])
    if (count > MAX_FILE || BigInt(count) !== final.size || !stable || after.isSymbolicLink() || await fs.realpath(target) !== target) fail()
    const bytes = buffer.subarray(0, count)
    return { source: new TextDecoder('utf-8', { fatal: true }).decode(bytes), bytes: count, sha256: hash(bytes) }
  } finally { await file.close() }
}
export async function snapshotGraph(inputRoot, entry) {
  const root = path.resolve(inputRoot)
  if (!(await fs.lstat(root)).isDirectory() || await fs.realpath(root) !== root) fail()
  if (typeof entry !== 'string' || entry !== path.posix.normalize(entry) || /[\\\0?#%:]/.test(entry) || !within(entry) || !entry.endsWith('.js') || entry.split('/').length > 8) fail()
  const modules = new Map()
  const queue = [entry]
  let total = 0
  while (queue.length) {
    const name = queue.pop()
    if (modules.has(name)) continue
    if (modules.size >= MAX_FILES) fail()
    const file = await readModule(root, name)
    total += file.bytes
    if (total > MAX_TOTAL) fail()
    const imports = dependencies(file.source).map(specifier => Object.freeze({ specifier, target: resolve(name, specifier) }))
    modules.set(name, Object.freeze({ ...file, imports: Object.freeze(imports) }))
    queue.push(...imports.map(item => item.target))
  }
  return { entry, modules }
}
export async function bundleSnapshot(graph) {
  // Copy data once, then every resolver/load operation reads only this snapshot.
  const entry = graph.entry
  const modules = new Map([...graph.modules].map(([name, file]) => [name, Object.freeze({
    ...file, imports: Object.freeze(file.imports.map(item => Object.freeze({ ...item }))),
  })]))
  const result = await esbuild.build({
    entryPoints: [entry], bundle: true, write: false, format: 'iife',
    globalName: 'UiPilotProbeModule', banner: { js: '"use strict";' }, platform: 'neutral', target: 'es2022',
    treeShaking: false, ignoreAnnotations: true, metafile: true,
    logLevel: 'silent', plugins: [{
      name: 'snapshot-only', setup(build) {
        build.onResolve({ filter: /.*/ }, args => {
          let name
          if (args.kind === 'entry-point' && args.namespace === '' && args.path === entry) name = entry
          else if (args.namespace === 'snapshot' && args.kind === 'import-statement') {
            name = modules.get(args.importer)?.imports.find(item => item.specifier === args.path)?.target
          }
          if (!name || !modules.has(name)) return { errors: [{ text: 'moduleGraphRejected' }] }
          return { path: name, namespace: 'snapshot' }
        })
        build.onLoad({ filter: /.*/ }, args => {
          const file = args.namespace === 'snapshot' ? modules.get(args.path) : null
          if (!file) return { errors: [{ text: 'moduleGraphRejected' }] }
          return { contents: file.source, loader: 'js' }
        })
      },
    }],
  })
  const code = result.outputFiles[0].text
  if (Buffer.byteLength(code) > 2 * MAX_TOTAL) fail()
  return { code, sha256: hash(code), inputs: [...modules].map(([name, file]) => ({ name, sha256: file.sha256, bytes: file.bytes, imports: file.imports })), compiler: esbuild.version, parser: parserVersion }
}
