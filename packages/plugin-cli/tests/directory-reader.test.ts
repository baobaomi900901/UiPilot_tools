import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { afterEach, describe, expect, it } from 'vitest'

import { readDirectorySnapshot } from '../src/directory-reader.js'
import type { WindowsAttributeProbe } from '../src/windows-attributes.js'

const roots: string[] = []

async function packageRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), 'uipilot-cli-directory-'))
  roots.push(root)
  await mkdir(join(root, 'dist'))
  await writeFile(join(root, 'plugin.json'), '{}')
  await writeFile(join(root, 'dist', 'runtime.js'), 'export {}')
  return root
}

afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })))
})

describe('readDirectorySnapshot', () => {
  it('builds a bounded immutable snapshot without modifying the source', async () => {
    const root = await packageRoot()
    const snapshot = await readDirectorySnapshot(root, { hostPlatform: 'linux' })
    expect([...snapshot.files.keys()]).toEqual(['dist/runtime.js', 'plugin.json'])
    expect(snapshot.files.get('dist/runtime.js')?.toString('utf8')).toBe('export {}')
  })

  it('fails closed when the Windows attribute adapter reports a reparse point', async () => {
    const root = await packageRoot()
    const probe: WindowsAttributeProbe = async (_root, paths) =>
      new Map(paths.map((path) => [path, path === 'dist/runtime.js' ? 0x400 : 0x20]))

    await expect(readDirectorySnapshot(root, { hostPlatform: 'win32', probe })).rejects.toMatchObject({
      code: 'SOURCE_INVALID',
    })
  })

  it('fails closed when the Windows adapter omits an entry', async () => {
    const root = await packageRoot()
    const probe: WindowsAttributeProbe = async () => new Map([['', 0x10]])
    await expect(readDirectorySnapshot(root, { hostPlatform: 'win32', probe })).rejects.toMatchObject({
      code: 'SOURCE_INVALID',
    })
  })
})
