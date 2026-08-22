import { lstat, open } from 'node:fs/promises'
import { resolve } from 'node:path'

import { readArchiveSnapshot } from './archive-reader.js'
import type { PluginPlatform, PluginValidationReportV1, ValidationRequest } from './contracts.js'
import { readDirectorySnapshot } from './directory-reader.js'
import { PACKAGE_LIMITS, PackagePolicyError } from './package-policy.js'
import { validateSnapshot } from './validate.js'
import {
  assertNoReparsePoints,
  createWindowsAttributeProbe,
  type WindowsAttributeProbe,
} from './windows-attributes.js'

export interface SourceValidationOptions {
  hostPlatform?: NodeJS.Platform
  probe?: WindowsAttributeProbe
}

function failure(
  source: string,
  platform: PluginPlatform,
  kind: 'directory' | 'archive' | 'unknown',
  code: PluginValidationReportV1['issues'][number]['code'],
  message: string,
): PluginValidationReportV1 {
  return {
    schemaVersion: 1,
    valid: false,
    source: { kind, path: source },
    target: { platform, hostVersion: '0.2.0', apiVersion: 1 },
    truncated: false,
    issues: [{ code, message }],
  }
}

function isNodeSystemError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && typeof (error as NodeJS.ErrnoException).code === 'string'
}

function sameFile(
  left: Awaited<ReturnType<typeof lstat>>,
  right: Awaited<ReturnType<typeof lstat>>,
): boolean {
  return (
    left.isFile() &&
    right.isFile() &&
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeMs === right.mtimeMs
  )
}

async function readArchiveFile(
  source: string,
  platform: NodeJS.Platform,
  injectedProbe?: WindowsAttributeProbe,
): Promise<Buffer> {
  const before = await lstat(source)
  if (!before.isFile() || before.isSymbolicLink() || before.size > PACKAGE_LIMITS.maxArchiveBytes) {
    throw new PackagePolicyError('SOURCE_INVALID', 'Archive source is unsafe.')
  }
  const probe = platform === 'win32' ? injectedProbe ?? createWindowsAttributeProbe() : undefined
  if (probe) assertNoReparsePoints([''], await probe(source, ['']))
  const handle = await open(source, 'r')
  try {
    const handleBefore = await handle.stat()
    if (!sameFile(before, handleBefore)) throw new PackagePolicyError('SOURCE_INVALID', 'Archive changed.')
    const bytes = await handle.readFile()
    const [handleAfter, after] = await Promise.all([handle.stat(), lstat(source)])
    if (!sameFile(handleBefore, handleAfter) || !sameFile(before, after)) {
      throw new PackagePolicyError('SOURCE_INVALID', 'Archive changed.')
    }
    if (probe) assertNoReparsePoints([''], await probe(source, ['']))
    return bytes
  } finally {
    await handle.close()
  }
}

export async function validatePackage(
  request: ValidationRequest,
  options: SourceValidationOptions = {},
): Promise<PluginValidationReportV1> {
  const { source, platform } = request
  const filesystemSource = resolve(source)
  const hostPlatform = options.hostPlatform ?? process.platform
  let metadata: Awaited<ReturnType<typeof lstat>>
  try {
    metadata = await lstat(filesystemSource)
  } catch {
    return failure(source, platform, 'unknown', 'SOURCE_INVALID', 'Source does not exist or is unreadable.')
  }
  if (metadata.isSymbolicLink()) {
    return failure(source, platform, 'unknown', 'SOURCE_INVALID', 'Source links are not allowed.')
  }

  let kind: 'directory' | 'archive' | 'unknown' = 'unknown'
  try {
    if (metadata.isDirectory()) {
      kind = 'directory'
      const snapshot = await readDirectorySnapshot(filesystemSource, { hostPlatform, probe: options.probe })
      return validateSnapshot(snapshot, source, platform)
    }
    if (!metadata.isFile() || !filesystemSource.endsWith('.uipilot-plugin')) {
      return failure(source, platform, 'unknown', 'SOURCE_INVALID', 'Source must be a plugin directory or .uipilot-plugin file.')
    }
    kind = 'archive'
    const bytes = await readArchiveFile(filesystemSource, hostPlatform, options.probe)
    return validateSnapshot(readArchiveSnapshot(bytes), source, platform)
  } catch (error) {
    if (error instanceof PackagePolicyError) {
      const reportedKind = error.code === 'SOURCE_INVALID' ? (kind === 'archive' ? 'archive' : 'unknown') : kind
      return failure(source, platform, reportedKind, error.code, error.message)
    }
    if (isNodeSystemError(error)) {
      return failure(source, platform, kind, kind === 'archive' ? 'ARCHIVE_INVALID' : 'SOURCE_INVALID', 'Source could not be validated.')
    }
    throw error
  }
}
