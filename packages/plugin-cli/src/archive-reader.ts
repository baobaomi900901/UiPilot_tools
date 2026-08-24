import { inflateRawSync } from 'node:zlib'

import { crc32 } from './crc32.js'
import {
  PACKAGE_LIMITS,
  PackagePolicyError,
  SnapshotBuilder,
  type PackageSnapshot,
} from './package-policy.js'

const EOCD_SIGNATURE = 0x06054b50
const CENTRAL_SIGNATURE = 0x02014b50
const LOCAL_SIGNATURE = 0x04034b50
const UTF8 = new TextDecoder('utf-8', { fatal: true })

interface CentralEntry {
  rawName: Buffer
  name: string
  directory: boolean
  flags: number
  method: number
  checksum: number
  compressedSize: number
  uncompressedSize: number
  localOffset: number
}

function invalid(message = 'Archive is invalid.'): PackagePolicyError {
  return new PackagePolicyError('ARCHIVE_INVALID', message)
}

function u16(bytes: Buffer, offset: number): number {
  if (offset < 0 || offset + 2 > bytes.length) throw invalid()
  return bytes.readUInt16LE(offset)
}

function u32(bytes: Buffer, offset: number): number {
  if (offset < 0 || offset + 4 > bytes.length) throw invalid()
  return bytes.readUInt32LE(offset)
}

function checkedEnd(offset: number, ...lengths: number[]): number {
  let end = offset
  for (const length of lengths) {
    if (!Number.isSafeInteger(length) || length < 0 || end > Number.MAX_SAFE_INTEGER - length) throw invalid()
    end += length
  }
  return end
}

function findEnd(bytes: Buffer): number {
  const earliest = Math.max(0, bytes.length - (0xffff + 22))
  for (let offset = bytes.length - 22; offset >= earliest; offset -= 1) {
    if (u32(bytes, offset) !== EOCD_SIGNATURE) continue
    const commentLength = u16(bytes, offset + 20)
    if (offset + 22 + commentLength === bytes.length) return offset
  }
  throw invalid('Archive end record is missing.')
}

function parseCentralEntries(bytes: Buffer): { entries: CentralEntry[]; centralOffset: number } {
  const endOffset = findEnd(bytes)
  const disk = u16(bytes, endOffset + 4)
  const centralDisk = u16(bytes, endOffset + 6)
  const entriesOnDisk = u16(bytes, endOffset + 8)
  const entryCount = u16(bytes, endOffset + 10)
  const centralSize = u32(bytes, endOffset + 12)
  const centralOffset = u32(bytes, endOffset + 16)
  if (
    disk !== 0 ||
    centralDisk !== 0 ||
    entriesOnDisk !== entryCount ||
    entryCount > PACKAGE_LIMITS.maxArchiveEntries ||
    entryCount === 0xffff ||
    centralSize === 0xffffffff ||
    centralOffset === 0xffffffff ||
    checkedEnd(centralOffset, centralSize) !== endOffset
  ) {
    throw invalid()
  }

  const entries: CentralEntry[] = []
  let offset = centralOffset
  for (let index = 0; index < entryCount; index += 1) {
    if (u32(bytes, offset) !== CENTRAL_SIGNATURE) throw invalid()
    const madeBy = u16(bytes, offset + 4)
    const flags = u16(bytes, offset + 8)
    const method = u16(bytes, offset + 10)
    const checksum = u32(bytes, offset + 16)
    const compressedSize = u32(bytes, offset + 20)
    const uncompressedSize = u32(bytes, offset + 24)
    const nameLength = u16(bytes, offset + 28)
    const extraLength = u16(bytes, offset + 30)
    const commentLength = u16(bytes, offset + 32)
    const diskStart = u16(bytes, offset + 34)
    const externalAttributes = u32(bytes, offset + 38)
    const localOffset = u32(bytes, offset + 42)
    const recordEnd = checkedEnd(offset, 46, nameLength, extraLength, commentLength)
    if (
      recordEnd > endOffset ||
      diskStart !== 0 ||
      compressedSize === 0xffffffff ||
      uncompressedSize === 0xffffffff ||
      localOffset === 0xffffffff ||
      (flags & 1) !== 0 ||
      (method !== 0 && method !== 8) ||
      uncompressedSize > PACKAGE_LIMITS.maxFileBytes
    ) {
      throw invalid()
    }
    const rawName = bytes.subarray(offset + 46, offset + 46 + nameLength)
    let name: string
    try {
      name = UTF8.decode(rawName)
    } catch {
      throw invalid('Archive filename is not UTF-8.')
    }
    if (!name || name.includes('\0')) throw invalid()
    const directory = name.endsWith('/')
    const creatorSystem = madeBy >>> 8
    if (creatorSystem === 3) {
      const mode = externalAttributes >>> 16
      const type = mode & 0o170000
      const expected = directory ? 0o040000 : 0o100000
      if (type !== 0 && type !== expected) throw invalid('Archive contains a special Unix entry.')
    }
    if (directory && (compressedSize !== 0 || uncompressedSize !== 0)) throw invalid()
    entries.push({
      rawName: Buffer.from(rawName),
      name,
      directory,
      flags,
      method,
      checksum,
      compressedSize,
      uncompressedSize,
      localOffset,
    })
    offset = recordEnd
  }
  if (offset !== endOffset) throw invalid()
  return { entries, centralOffset }
}

function readEntry(bytes: Buffer, entry: CentralEntry, centralOffset: number): { start: number; end: number; data: Buffer } {
  const offset = entry.localOffset
  if (u32(bytes, offset) !== LOCAL_SIGNATURE) throw invalid()
  const flags = u16(bytes, offset + 6)
  const method = u16(bytes, offset + 8)
  const localChecksum = u32(bytes, offset + 14)
  const localCompressedSize = u32(bytes, offset + 18)
  const localUncompressedSize = u32(bytes, offset + 22)
  const nameLength = u16(bytes, offset + 26)
  const extraLength = u16(bytes, offset + 28)
  const dataOffset = checkedEnd(offset, 30, nameLength, extraLength)
  const dataEnd = checkedEnd(dataOffset, entry.compressedSize)
  if (dataEnd > centralOffset || flags !== entry.flags || method !== entry.method) throw invalid()
  const localName = bytes.subarray(offset + 30, offset + 30 + nameLength)
  if (!localName.equals(entry.rawName)) throw invalid()
  const descriptor = (flags & 8) !== 0
  if (
    (!descriptor &&
      (localChecksum !== entry.checksum ||
        localCompressedSize !== entry.compressedSize ||
        localUncompressedSize !== entry.uncompressedSize)) ||
    (descriptor &&
      ((localChecksum !== 0 && localChecksum !== entry.checksum) ||
        (localCompressedSize !== 0 && localCompressedSize !== entry.compressedSize) ||
        (localUncompressedSize !== 0 && localUncompressedSize !== entry.uncompressedSize)))
  ) {
    throw invalid()
  }

  const compressed = bytes.subarray(dataOffset, dataEnd)
  let data: Buffer
  try {
    data = entry.method === 0
      ? Buffer.from(compressed)
      : inflateRawSync(compressed, { maxOutputLength: PACKAGE_LIMITS.maxFileBytes + 1 })
  } catch {
    throw invalid('Archive entry cannot be decompressed.')
  }
  if (data.byteLength !== entry.uncompressedSize || crc32(data) !== entry.checksum) {
    throw invalid('Archive entry size or CRC is invalid.')
  }
  return { start: offset, end: dataEnd, data }
}

export function readArchiveSnapshot(input: Uint8Array): PackageSnapshot {
  const bytes = Buffer.from(input)
  if (bytes.byteLength > PACKAGE_LIMITS.maxArchiveBytes) {
    throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', 'Archive is too large.')
  }
  const { entries, centralOffset } = parseCentralEntries(bytes)
  const builder = new SnapshotBuilder('archive')
  const intervals: Array<{ start: number; end: number }> = []
  for (const entry of entries) {
    if (entry.directory) {
      builder.addDirectory(entry.name.slice(0, -1))
      continue
    }
    const loaded = readEntry(bytes, entry, centralOffset)
    intervals.push({ start: loaded.start, end: loaded.end })
    builder.addFile(entry.name, loaded.data)
  }
  intervals.sort((left, right) => left.start - right.start)
  for (let index = 1; index < intervals.length; index += 1) {
    if (intervals[index].start < intervals[index - 1].end) throw invalid('Archive entries overlap.')
  }
  return builder.finish()
}
