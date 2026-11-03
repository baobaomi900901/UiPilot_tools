import { describe, expect, it } from 'vitest'

import { readArchiveSnapshot } from '../src/archive-reader.js'
import { createZip } from './zip-fixture.js'

const manifest = Buffer.from('{}')
const runtime = Buffer.from('export {}')

describe('readArchiveSnapshot', () => {
  it.each([0, 8] as const)('accepts method %s and verifies content', (method) => {
    const snapshot = readArchiveSnapshot(
      createZip([
        { name: 'plugin.json', data: manifest, method },
        { name: 'dist/runtime.js', data: runtime, method },
      ]),
    )
    expect(snapshot.files.get('dist/runtime.js')?.equals(runtime)).toBe(true)
  })

  it.each([0, 8] as const)('rejects method %s with an incorrect central CRC', (method) => {
    expect(() =>
      readArchiveSnapshot(
        createZip([{ name: 'plugin.json', data: manifest, method, crcOverride: 0x12345678 }]),
      ),
    ).toThrowError(expect.objectContaining({ code: 'ARCHIVE_INVALID' }))
  })

  it('rejects a Unix symlink entry', () => {
    expect(() =>
      readArchiveSnapshot(
        createZip([
          { name: 'plugin.json', data: manifest },
          { name: 'dist/link.js', data: Buffer.from('runtime.js'), unixMode: 0o120777 },
        ]),
      ),
    ).toThrowError(expect.objectContaining({ code: 'ARCHIVE_INVALID' }))
  })

  it('rejects traversal and case-fold collisions', () => {
    for (const archive of [
      createZip([{ name: '../escape.js', data: runtime }]),
      createZip([
        { name: 'Dist/a.js', data: runtime },
        { name: 'dist/b.js', data: runtime },
      ]),
    ]) {
      expect(() => readArchiveSnapshot(archive)).toThrow()
    }
  })
})
