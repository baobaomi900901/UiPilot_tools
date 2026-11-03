import { deflateRawSync } from 'node:zlib'

import { crc32 } from '../src/crc32.js'

export interface ZipFixtureEntry {
  name: string
  data?: Buffer
  method?: 0 | 8
  unixMode?: number
  crcOverride?: number
}

export function createZip(entries: readonly ZipFixtureEntry[]): Buffer {
  const localParts: Buffer[] = []
  const centralParts: Buffer[] = []
  let localOffset = 0

  for (const entry of entries) {
    const name = Buffer.from(entry.name, 'utf8')
    const data = entry.data ?? Buffer.alloc(0)
    const method = entry.method ?? 0
    const compressed = method === 8 ? deflateRawSync(data) : data
    const checksum = entry.crcOverride ?? crc32(data)
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50, 0)
    local.writeUInt16LE(20, 4)
    local.writeUInt16LE(0x800, 6)
    local.writeUInt16LE(method, 8)
    local.writeUInt32LE(checksum >>> 0, 14)
    local.writeUInt32LE(compressed.length, 18)
    local.writeUInt32LE(data.length, 22)
    local.writeUInt16LE(name.length, 26)

    const central = Buffer.alloc(46)
    central.writeUInt32LE(0x02014b50, 0)
    central.writeUInt16LE(entry.unixMode === undefined ? 20 : 0x0314, 4)
    central.writeUInt16LE(20, 6)
    central.writeUInt16LE(0x800, 8)
    central.writeUInt16LE(method, 10)
    central.writeUInt32LE(checksum >>> 0, 16)
    central.writeUInt32LE(compressed.length, 20)
    central.writeUInt32LE(data.length, 24)
    central.writeUInt16LE(name.length, 28)
    central.writeUInt32LE(((entry.unixMode ?? 0) << 16) >>> 0, 38)
    central.writeUInt32LE(localOffset, 42)

    localParts.push(local, name, compressed)
    centralParts.push(central, name)
    localOffset += local.length + name.length + compressed.length
  }

  const central = Buffer.concat(centralParts)
  const end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50, 0)
  end.writeUInt16LE(entries.length, 8)
  end.writeUInt16LE(entries.length, 10)
  end.writeUInt32LE(central.length, 12)
  end.writeUInt32LE(localOffset, 16)
  return Buffer.concat([...localParts, central, end])
}
