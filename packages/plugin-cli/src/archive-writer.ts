import { crc32 } from './crc32.js'
import { PACKAGE_LIMITS, PackagePolicyError, type PackageSnapshot } from './package-policy.js'

/** Encode an already policy-bounded snapshot without consulting the filesystem. */
export function writeArchiveSnapshot(snapshot: PackageSnapshot): Buffer {
  const entries = [
    ...snapshot.directories.map((path) => ({ name: Buffer.from(`${path}/`), data: Buffer.alloc(0), directory: true })),
    ...[...snapshot.files].map(([path, data]) => ({ name: Buffer.from(path), data, directory: false })),
  ].sort((left, right) => Buffer.compare(left.name, right.name))
  const size = entries.reduce((total, entry) => total + 76 + 2 * entry.name.length + entry.data.length, 22)
  if (entries.length > PACKAGE_LIMITS.maxArchiveEntries || size > PACKAGE_LIMITS.maxArchiveBytes) {
    throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', 'Archive is too large.')
  }
  const local: Buffer[] = []
  const central: Buffer[] = []
  let offset = 0
  for (const entry of entries) {
    const checksum = crc32(entry.data)
    const header = Buffer.alloc(30)
    header.writeUInt32LE(0x04034b50, 0)
    header.writeUInt16LE(20, 4)
    header.writeUInt16LE(0x800, 6)
    header.writeUInt16LE(33, 12)
    header.writeUInt32LE(checksum, 14)
    header.writeUInt32LE(entry.data.length, 18)
    header.writeUInt32LE(entry.data.length, 22)
    header.writeUInt16LE(entry.name.length, 26)
    local.push(header, entry.name, entry.data)
    const record = Buffer.alloc(46)
    record.writeUInt32LE(0x02014b50, 0)
    record.writeUInt16LE(20, 4) // DOS creator, no platform metadata.
    record.writeUInt16LE(20, 6)
    record.writeUInt16LE(0x800, 8)
    record.writeUInt16LE(33, 14)
    record.writeUInt32LE(checksum, 16)
    record.writeUInt32LE(entry.data.length, 20)
    record.writeUInt32LE(entry.data.length, 24)
    record.writeUInt16LE(entry.name.length, 28)
    record.writeUInt32LE(entry.directory ? 0x10 : 0, 38)
    record.writeUInt32LE(offset, 42)
    central.push(record, entry.name)
    offset += header.length + entry.name.length + entry.data.length
  }
  const end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50, 0)
  end.writeUInt16LE(entries.length, 8)
  end.writeUInt16LE(entries.length, 10)
  end.writeUInt32LE(size - offset - 22, 12)
  end.writeUInt32LE(offset, 16)
  return Buffer.concat([...local, ...central, end], size)
}
