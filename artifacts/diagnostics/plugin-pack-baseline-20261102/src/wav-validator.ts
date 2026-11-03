const MAX_ALARM_BYTES = 2 * 1024 * 1024
const MAX_ALARM_SECONDS = 15

export class WavValidationError extends Error {
  constructor(message = 'Timer alarm WAV is invalid.') {
    super(message)
    this.name = 'WavValidationError'
  }
}

function invalid(): never {
  throw new WavValidationError()
}

export function validateTimerAlarmWav(input: Uint8Array): void {
  const bytes = Buffer.from(input)
  if (bytes.byteLength === 0 || bytes.byteLength > MAX_ALARM_BYTES || bytes.byteLength < 44) invalid()
  if (
    bytes.toString('ascii', 0, 4) !== 'RIFF' ||
    bytes.toString('ascii', 8, 12) !== 'WAVE' ||
    bytes.readUInt32LE(4) + 8 !== bytes.length ||
    bytes.toString('ascii', 12, 16) !== 'fmt ' ||
    bytes.readUInt32LE(16) !== 16 ||
    bytes.toString('ascii', 36, 40) !== 'data'
  ) {
    invalid()
  }
  const format = bytes.readUInt16LE(20)
  const channels = bytes.readUInt16LE(22)
  const sampleRate = bytes.readUInt32LE(24)
  const byteRate = bytes.readUInt32LE(28)
  const blockAlign = bytes.readUInt16LE(32)
  const bitsPerSample = bytes.readUInt16LE(34)
  const dataLength = bytes.readUInt32LE(40)
  const padding = dataLength % 2
  if (
    44 + dataLength + padding !== bytes.length ||
    (padding === 1 && bytes.at(-1) !== 0) ||
    format !== 1 ||
    (channels !== 1 && channels !== 2) ||
    (sampleRate !== 44_100 && sampleRate !== 48_000) ||
    (bitsPerSample !== 16 && bitsPerSample !== 24)
  ) {
    invalid()
  }
  const bytesPerSample = bitsPerSample / 8
  const expectedAlign = channels * bytesPerSample
  const expectedRate = sampleRate * expectedAlign
  if (blockAlign !== expectedAlign || byteRate !== expectedRate || dataLength % blockAlign !== 0) invalid()
  const frames = dataLength / blockAlign
  if (frames === 0 || frames > sampleRate * MAX_ALARM_SECONDS) invalid()
}
