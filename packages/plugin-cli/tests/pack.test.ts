import { link, mkdtemp, mkdir, readFile, readdir, rm, symlink, unlink, utimes, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { packPackage } from '../src/pack.js'
import { readArchiveSnapshot } from '../src/archive-reader.js'
import { validateSnapshot } from '../src/validate.js'
import { SnapshotBuilder } from '../src/package-policy.js'

import { multiCommandManifest } from './multi-command-fixture.js'

const roots: string[] = []
const options = { hostPlatform: 'linux' as const }
async function setup() {
  const root = await mkdtemp(join(tmpdir(), 'uipilot pack '))
  roots.push(root)
  const source = join(root, 'source')
  await mkdir(join(source, 'dist'), { recursive: true })
  await mkdir(join(source, 'empty'))
  await writeFile(join(source, 'dist/runtime.js'), 'export {}\n')
  await writeFile(join(source, 'plugin.json'), JSON.stringify({
    schemaVersion: 1, pluginId: 'com.example.pack', version: '1.0.0', apiVersion: 1,
    minimumHostVersion: '0.2.0', name: 'Pack', supportedPlatforms: ['windows', 'macos'],
    command: { defaultName: 'pack', activationMode: 'submit', outputMode: 'mainResult', inputRequired: false },
    runtime: { entry: 'dist/runtime.js' }, permissions: [], settings: [],
  }))
  return { root, source, output: join(root, 'result.uipilot-plugin'), platform: 'windows' as const }
}
afterEach(async () => { await Promise.all(roots.splice(0).map(root => rm(root, { recursive: true, force: true }))) })
const systemError = (code: string) => Object.assign(new Error('injected failure'), { code })

describe('packPackage', () => {
  it('publishes a validated archive with SHA-256 and absolute output path', async () => {
    const request = await setup()
    const result = await packPackage(request, options)
    const bytes = await readFile(request.output)
    expect(result).toMatchObject({ schemaVersion: 1, created: true, output: { path: resolve(request.output), sha256: createHash('sha256').update(bytes).digest('hex') }, validation: { valid: true } })
    expect(result).not.toHaveProperty('warnings')
    expect(validateSnapshot(readArchiveSnapshot(bytes), request.output, 'windows').valid).toBe(true)
    expect(await readdir(request.root)).toEqual(['result.uipilot-plugin', 'source'])
  })
  it('publishes using the native filesystem and platform attribute adapter', async () => {
    const request = await setup()
    expect(await packPackage(request)).toMatchObject({ created: true, validation: { valid: true } })
  }, 30_000)
  it('ignores source location and mtimes but hashes changed bytes', async () => {
    const first = await setup(), second = await setup()
    await utimes(join(second.source, 'dist/runtime.js'), new Date(0), new Date(0))
    const a = await packPackage(first, options), b = await packPackage(second, options)
    expect(await readFile(first.output)).toEqual(await readFile(second.output))
    expect(a.output).toHaveProperty('sha256', b.created ? b.output.sha256 : '')
    await unlink(second.output)
    await writeFile(join(second.source, 'dist/runtime.js'), 'changed')
    const c = await packPackage(second, options)
    expect(c.output).not.toEqual(b.output)
  })
  it('archives the same validated snapshot without rereading changed source content', async () => {
    const request = await setup()
    const result = await packPackage(request, { ...options, publication: { write: async (handle, bytes) => {
      await writeFile(join(request.source, 'dist/runtime.js'), 'changed after snapshot')
      await handle.writeFile(bytes)
    } } })
    expect(result.created).toBe(true)
    expect(readArchiveSnapshot(await readFile(request.output)).files.get('dist/runtime.js')?.toString()).toBe('export {}\n')
  })
  it('rejects invalid manifests without creating any output', async () => {
    const request = await setup()
    await writeFile(join(request.source, 'plugin.json'), '{}')
    expect(await packPackage(request, options)).toMatchObject({ created: false, error: { code: 'PACKAGE_INVALID' }, validation: { valid: false } })
    expect(await readdir(request.root)).toEqual(['source'])
  })
  it('rejects unsafe resources and nonexistent sources without writing', async () => {
    const request = await setup()
    await writeFile(join(request.source, 'secret.txt'), 'fixture')
    expect(await packPackage(request, options)).toMatchObject({ created: false, error: { code: 'PACKAGE_INVALID' } })
    expect(await packPackage({ ...request, source: join(request.root, 'missing') }, options)).toMatchObject({ created: false, error: { code: 'SOURCE_INVALID' } })
    expect(await readdir(request.root)).toEqual(['source'])
  })
  it.each(['file', 'directory', 'link'])('preserves an existing output %s', async kind => {
    const request = await setup()
    if (kind === 'file') await writeFile(request.output, 'existing')
    else if (kind === 'directory') await mkdir(request.output)
    else await symlink(request.source, request.output, 'junction')
    expect(await packPackage(request, options)).toMatchObject({ created: false, error: { code: 'OUTPUT_EXISTS' } })
    if (kind === 'file') expect(await readFile(request.output, 'utf8')).toBe('existing')
  })
  it('rejects inner output, wrong suffix, and absent parents', async () => {
    const request = await setup()
    for (const output of [join(request.source, 'inner.uipilot-plugin'), join(request.root, 'bad.UIPILOT-PLUGIN'), join(request.root, 'absent/out.uipilot-plugin')]) {
      expect(await packPackage({ ...request, output }, options)).toMatchObject({ created: false, error: { code: 'OUTPUT_INVALID' } })
    }
  })
  it('rejects static ancestor aliases for both output and source containment', async () => {
    const request = await setup()
    const alias = join(request.root, 'alias')
    await symlink(request.source, alias, 'junction')
    expect(await packPackage({ ...request, output: join(alias, 'out.uipilot-plugin') }, options)).toMatchObject({ created: false, error: { code: 'OUTPUT_INVALID' } })
    const parentAlias = join(request.root, 'parent-alias')
    await symlink(request.root, parentAlias, 'junction')
    expect(await packPackage({ ...request, source: join(parentAlias, 'source'), output: join(request.source, 'out.uipilot-plugin') }, options)).toMatchObject({ created: false, error: { code: 'OUTPUT_INVALID' } })
  })
  it('rejects linked source contents', async () => {
    const request = await setup()
    await symlink(request.source, join(request.source, 'loop'), 'junction')
    expect(await packPackage(request, options)).toMatchObject({ created: false, error: { code: 'SOURCE_INVALID' } })
  })
  it('fails closed on Windows ancestor reparse attributes', async () => {
    const request = await setup()
    const probe = async (root: string, paths: readonly string[]) => new Map(paths.map(path => [path, root === dirname(request.output) ? 0x400 : 0x10]))
    expect(await packPackage(request, { hostPlatform: 'win32', probe })).toMatchObject({ created: false, error: { code: 'OUTPUT_INVALID' } })
  })
  it('preserves a competing destination and cleans only its own temporary file', async () => {
    const request = await setup()
    await writeFile(join(request.root, 'unrelated.tmp'), 'keep')
    const result = await packPackage(request, { ...options, publication: { link: async (from, to) => { await writeFile(to, 'competitor'); await link(from, to) } } })
    expect(result).toMatchObject({ created: false, error: { code: 'OUTPUT_EXISTS' } })
    expect(await readFile(request.output, 'utf8')).toBe('competitor')
    expect(await readdir(request.root)).toEqual(['result.uipilot-plugin', 'source', 'unrelated.tmp'])
  })
  it('classifies a raced directory even when hard-link reports EPERM', async () => {
    const request = await setup()
    const result = await packPackage(request, { ...options, publication: { link: async (_from, to) => { await mkdir(to); throw systemError('EPERM') } } })
    expect(result).toMatchObject({ created: false, error: { code: 'OUTPUT_EXISTS' } })
    expect(await readdir(request.output)).toEqual([])
    expect(await readdir(request.root)).toEqual(['result.uipilot-plugin', 'source'])
  })
  it('resolves source and output relative to the current working directory', async () => {
    const request = await setup()
    const result = await packPackage({ ...request, source: relative(process.cwd(), request.source), output: relative(process.cwd(), request.output) }, options)
    expect(result).toMatchObject({ created: true, output: { path: request.output } })
  })
  it('cleans its owned temp on write failure', async () => {
    const request = await setup()
    const result = await packPackage(request, { ...options, publication: { write: async () => { throw systemError('ENOSPC') } } })
    expect(result).toMatchObject({ created: false, error: { code: 'OUTPUT_WRITE_FAILED' } })
    expect(await readdir(request.root)).toEqual(['source'])
  })
  it('returns success plus warning when published temp cleanup fails', async () => {
    const request = await setup()
    const result = await packPackage(request, { ...options, publication: { unlink: async () => { throw systemError('EPERM') } } })
    expect(result).toMatchObject({ created: true, warnings: [{ code: 'TEMP_CLEANUP_FAILED' }] })
    expect(readArchiveSnapshot(await readFile(request.output)).files.has('plugin.json')).toBe(true)
  })
  it('does not claim ownership of a temp it failed to create', async () => {
    const request = await setup()
    let removed = false
    const result = await packPackage(request, { ...options, publication: { open: async () => { throw systemError('EEXIST') }, unlink: async () => { removed = true } } })
    expect(result).toMatchObject({ created: false, error: { code: 'OUTPUT_WRITE_FAILED' } })
    expect(removed).toBe(false)
  })
  it('propagates unknown errors to CLI_INTERNAL handling', async () => {
    const request = await setup()
    await expect(packPackage(request, { ...options, publication: { write: async () => { throw new Error('unknown') } } })).rejects.toThrow('unknown')
    expect(await readdir(request.root)).toEqual(['source'])
  })
})


it('embeds the same per-command directory diagnostics and metadata in a production-incompatible pack', async () => {
  const request = await setup()
  const manifest = multiCommandManifest()
  manifest.minimumHostVersion = '0.8.1'
  manifest.commands[0].summary = ''
  manifest.commands[1].inputRequired = true
  await writeFile(join(request.source, 'plugin.json'), JSON.stringify(manifest))
  const result = await packPackage(request, options)
  const snapshot = new SnapshotBuilder('directory')
  snapshot.addFile('plugin.json', Buffer.from(JSON.stringify(manifest)))
  snapshot.addFile('dist/runtime.js', Buffer.from('export {}\n'))
  const expected = validateSnapshot(snapshot.finish(), resolve(request.source), 'windows')
  expect(result).toMatchObject({ created: false, error: { code: 'PACKAGE_INVALID' } })
  expect(result.validation).toEqual(expected)
  expect(result.validation?.issues.map(issue => issue.commandId)).toEqual(['upper', 'constructor', undefined])
  expect(await readdir(request.root)).toEqual(['source'])
})


it.each([false, true])('production Host 0.6 packs complete commands arrays (single: %s)', async (single) => {
  const request = await setup()
  const manifest = multiCommandManifest()
  if (single) manifest.commands = manifest.commands.slice(0, 1)
  await writeFile(join(request.source, 'plugin.json'), JSON.stringify(manifest))
  const result = await packPackage(request, options)
  expect(result).toMatchObject({ created: true, validation: { valid: true, target: { hostVersion: '0.8.0' } } })
  if (!result.created) throw new Error('Expected production pack success')
  expect(result.validation?.plugin?.commands?.map(command => command.commandId)).toEqual(manifest.commands.map(command => command.id))
  const bytes = await readFile(request.output)
  const archive = validateSnapshot(readArchiveSnapshot(bytes), request.output, 'windows')
  expect({ ...archive, source: result.validation?.source }).toEqual(result.validation)
  const second = await packPackage({ ...request, output: join(request.root, 'second.uipilot-plugin') }, options)
  expect(second.created).toBe(true)
  if (!second.created) throw new Error('Expected repeated pack success')
  expect(await readFile(join(request.root, 'second.uipilot-plugin'))).toEqual(bytes)
  expect(second.output.sha256).toBe(result.output.sha256)
})
