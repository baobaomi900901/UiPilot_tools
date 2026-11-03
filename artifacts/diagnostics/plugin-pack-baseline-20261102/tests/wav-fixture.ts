export function createWav(
  frames: number,
  channels: 1 | 2 = 1,
  sampleRate: 44_100 | 48_000 = 44_100,
  bitsPerSample: 16 | 24 = 16,
): Buffer {
  const blockAlign = channels * (bitsPerSample / 8)
  const byteRate = sampleRate * blockAlign
  const dataLength = frames * blockAlign
  const padding = dataLength % 2
  const bytes = Buffer.alloc(44 + dataLength + padding)
  bytes.write('RIFF', 0, 'ascii')
  bytes.writeUInt32LE(bytes.length - 8, 4)
  bytes.write('WAVEfmt ', 8, 'ascii')
  bytes.writeUInt32LE(16, 16)
  bytes.writeUInt16LE(1, 20)
  bytes.writeUInt16LE(channels, 22)
  bytes.writeUInt32LE(sampleRate, 24)
  bytes.writeUInt32LE(byteRate, 28)
  bytes.writeUInt16LE(blockAlign, 32)
  bytes.writeUInt16LE(bitsPerSample, 34)
  bytes.write('data', 36, 'ascii')
  bytes.writeUInt32LE(dataLength, 40)
  return bytes
}
