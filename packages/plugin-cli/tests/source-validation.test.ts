import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { isAbsolute, join } from 'node:path'

import { afterEach, describe, expect, it } from 'vitest'

import { validatePackage } from '../src/source-validation.js'
import { createZip } from './zip-fixture.js'

const roots: string[] = []

function manifest(): Buffer {
  return Buffer.from(
    JSON.stringify({
      schemaVersion: 1,
      pluginId: 'com.example.result',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.2.0',
      name: 'Result',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'result',
        activationMode: 'submit',
        outputMode: 'mainResult',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      permissions: [],
      settings: [],
    }),
  )
}

async function root(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), 'uipilot-cli-source-'))
  roots.push(path)
  return path
}

afterEach(async () => {
  await Promise.all(roots.splice(0).map((path) => rm(path, { recursive: true, force: true })))
})

describe('validatePackage', () => {
  it('validates directory and archive sources through the same core', async () => {
    const base = await root()
    const directory = join(base, 'package')
    await mkdir(join(directory, 'dist'), { recursive: true })
    await writeFile(join(directory, 'plugin.json'), manifest())
    await writeFile(join(directory, 'dist', 'runtime.js'), 'export {}')
    await expect(
      validatePackage({ source: directory, platform: 'windows' }, { hostPlatform: 'linux' }),
    ).resolves.toMatchObject({ valid: true, source: { kind: 'directory', path: directory } })

    const archive = join(base, 'result.uipilot-plugin')
    await writeFile(
      archive,
      createZip([
        { name: 'plugin.json', data: manifest(), method: 8 },
        { name: 'dist/runtime.js', data: Buffer.from('export {}'), method: 8 },
      ]),
    )
    await expect(
      validatePackage({ source: archive, platform: 'windows' }, { hostPlatform: 'linux' }),
    ).resolves.toMatchObject({ valid: true, source: { kind: 'archive', path: archive } })
  })

  it('reports missing and wrong-extension sources as unknown validation failures', async () => {
    const base = await root()
    for (const source of [join(base, 'missing'), join(base, 'plugin.zip')]) {
      if (source.endsWith('.zip')) await writeFile(source, 'not a package')
      const report = await validatePackage(
        { source, platform: 'windows' },
        { hostPlatform: 'linux' },
      )
      expect(report).toMatchObject({
        valid: false,
        source: { kind: 'unknown', path: source },
        truncated: false,
        issues: [{ code: 'SOURCE_INVALID' }],
      })
    }
  })

  it('reports a malformed package archive without throwing', async () => {
    const base = await root()
    const source = join(base, 'broken.uipilot-plugin')
    await writeFile(source, 'not a zip')
    await expect(
      validatePackage({ source, platform: 'windows' }, { hostPlatform: 'linux' }),
    ).resolves.toMatchObject({
      valid: false,
      source: { kind: 'archive', path: source },
      issues: [{ code: 'ARCHIVE_INVALID' }],
    })
  })

  it('preserves a relative source while giving the Windows probe an absolute root', async () => {
    const source = join('..', '..', 'examples', 'public-plugins', 'com.uipilot.demo-return', 'package')
    const report = await validatePackage(
      { source, platform: 'windows' },
      {
        hostPlatform: 'win32',
        probe: async (root, paths) => {
          expect(isAbsolute(root)).toBe(true)
          return new Map(paths.map((path) => [path, 0]))
        },
      },
    )

    expect(report).toMatchObject({ valid: true, source: { kind: 'directory', path: source } })
  })

  it('does not disguise an unexpected implementation error as an invalid source', async () => {
    const source = join('..', '..', 'examples', 'public-plugins', 'com.uipilot.demo-return', 'package')
    await expect(
      validatePackage(
        { source, platform: 'windows' },
        {
          hostPlatform: 'win32',
          probe: async () => {
            throw new TypeError('implementation defect')
          },
        },
      ),
    ).rejects.toThrow(TypeError)
  })
})
