import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { writeArchiveSnapshot } from '../src/archive-writer.js'
import { readArchiveSnapshot } from '../src/archive-reader.js'
import { PACKAGE_LIMITS, SnapshotBuilder } from '../src/package-policy.js'

export function fixtureSnapshot() {
  const builder = new SnapshotBuilder('directory')
  builder.addDirectory('empty')
  builder.addDirectory('nested/empty')
  builder.addFile('dist/runtime.js', Buffer.from('export {}\n'))
  builder.addFile('plugin.json', Buffer.from(JSON.stringify({
    schemaVersion: 1, pluginId: 'com.example.cli-pack', version: '1.0.0', apiVersion: 1,
    minimumHostVersion: '0.2.0', name: 'CLI pack fixture', supportedPlatforms: ['windows', 'macos'],
    command: { defaultName: 'cli-pack', activationMode: 'submit', outputMode: 'mainResult', inputRequired: false },
    runtime: { entry: 'dist/runtime.js' }, permissions: [], settings: [],
  })))
  return builder.finish()
}

describe('writeArchiveSnapshot', () => {
  it('roundtrips files and explicit empty/nested directories deterministically', () => {
    const snapshot = fixtureSnapshot()
    const bytes = writeArchiveSnapshot(snapshot)
    const reversed = { ...snapshot, files: new Map([...snapshot.files].reverse()), directories: [...snapshot.directories].reverse() }
    expect(writeArchiveSnapshot(reversed)).toEqual(bytes)
    const roundtrip = readArchiveSnapshot(bytes)
    expect(roundtrip.files).toEqual(snapshot.files)
    expect(roundtrip.directories).toEqual(snapshot.directories)
    expect(bytes.readUInt16LE(6)).toBe(0x800)
    expect(bytes.readUInt16LE(10)).toBe(0)
    expect(bytes.readUInt16LE(12)).toBe(33)
  })
  it('preserves binary icon and alarm resources', () => {
    const builder = new SnapshotBuilder('directory')
    builder.addFile('icon.png', Buffer.from([0, 255, 17]))
    builder.addFile('assets/sounds/timer-alarm.wav', Buffer.from([82, 73, 70, 70, 0, 255]))
    const snapshot = builder.finish()
    expect(readArchiveSnapshot(writeArchiveSnapshot(snapshot)).files).toEqual(snapshot.files)
  })
  it('counts ZIP headers toward the archive byte limit', () => {
    const builder = new SnapshotBuilder('directory')
    for (let i = 0; i < 8; i++) builder.addFile(`${i}.js`, Buffer.alloc(PACKAGE_LIMITS.maxFileBytes))
    expect(() => writeArchiveSnapshot(builder.finish())).toThrowError(expect.objectContaining({ code: 'PACKAGE_LIMIT_EXCEEDED' }))
  })
  it('matches the checked-in cross-language fixture byte for byte', () => {
    const fixture = fileURLToPath(new URL('../../../tests/fixtures/public-plugin-pack/roundtrip.uipilot-plugin', import.meta.url))
    if (process.env.UPDATE_PACK_FIXTURE === '1') writeFileSync(fixture, writeArchiveSnapshot(fixtureSnapshot()))
    expect(writeArchiveSnapshot(fixtureSnapshot())).toEqual(readFileSync(fixture))
  })
})
