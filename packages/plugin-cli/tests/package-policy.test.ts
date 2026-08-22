import { describe, expect, it } from 'vitest'

import { PackagePolicyError, SnapshotBuilder, canonicalRelativePath } from '../src/package-policy.js'
import { foldName, normalizeNfc } from '../src/unicode.js'

describe('Unicode 15.1 package identity', () => {
  it('normalizes canonical Latin and algorithmic Hangul sequences', () => {
    expect(normalizeNfc('e\u0301')).toBe('\u00e9')
    expect(normalizeNfc('\u1100\u1161')).toBe('\uac00')
    expect(normalizeNfc('\uac00\u11a8')).toBe('\uac01')
  })

  it('uses full case folding followed by NFC', () => {
    expect(foldName('Stra\u00dfe')).toBe(foldName('STRASSE'))
    expect(foldName('\u212b')).toBe(foldName('A\u030a'))
  })
})

describe('package path policy', () => {
  it('accepts canonical package paths and the fixed timer alarm', () => {
    expect(canonicalRelativePath('dist/runtime.js')).toBe('dist/runtime.js')
    expect(canonicalRelativePath('assets/sounds/timer-alarm.wav')).toBe(
      'assets/sounds/timer-alarm.wav',
    )
  })

  it.each([
    '../escape.js',
    '/absolute.js',
    'dist\\runtime.js',
    'dist//runtime.js',
    'dist/e\u0301.js',
    'dist/CON.js',
    'dist/file.js.',
    'dist/data.json',
    'dist/other.wav',
  ])('rejects unsafe or unsupported path %s', (path) => {
    expect(() => canonicalRelativePath(path)).toThrow(PackagePolicyError)
  })

  it('rejects full-fold file and parent-directory collisions', () => {
    const files = new SnapshotBuilder('directory')
    files.addFile('dist/Stra\u00dfe.js', Buffer.from('a'))
    expect(() => files.addFile('dist/STRASSE.js', Buffer.from('b'))).toThrowError(
      expect.objectContaining({ code: 'PATH_COLLISION' }),
    )

    const directories = new SnapshotBuilder('directory')
    directories.addFile('Dist/a.js', Buffer.from('a'))
    expect(() => directories.addFile('dist/b.js', Buffer.from('b'))).toThrowError(
      expect.objectContaining({ code: 'PATH_COLLISION' }),
    )
  })
})
