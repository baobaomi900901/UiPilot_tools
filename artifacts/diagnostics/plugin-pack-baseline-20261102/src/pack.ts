import { createHash, randomUUID } from 'node:crypto'
import { link, lstat, open, realpath, unlink, type FileHandle } from 'node:fs/promises'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'

import { readArchiveSnapshot } from './archive-reader.js'
import { writeArchiveSnapshot } from './archive-writer.js'
import type {
  PackRequest,
  PluginPackErrorCode,
  PluginPackReportV1,
  PluginValidationReportV1,
} from './contracts.js'
import { readDirectorySnapshot, type DirectoryReaderOptions } from './directory-reader.js'
import { PackagePolicyError } from './package-policy.js'
import { validateSnapshot } from './validate.js'
import { assertNoReparsePoints, createWindowsAttributeProbe } from './windows-attributes.js'

interface PublicationOperations {
  open: (path: string, flags: 'wx') => Promise<FileHandle>
  write: (handle: FileHandle, bytes: Buffer) => Promise<void>
  link: (source: string, output: string) => Promise<void>
  unlink: (path: string) => Promise<void>
}

export interface PackOptions extends DirectoryReaderOptions {
  /** Narrow filesystem seam for publication fault injection; defaults use real atomic operations. */
  publication?: Partial<PublicationOperations>
}

const FILESYSTEM_ERRORS = new Set([
  'EACCES', 'EPERM', 'ENOENT', 'ENOTDIR', 'EISDIR', 'EEXIST', 'ENOSPC', 'EDQUOT', 'EROFS',
  'EMFILE', 'ENFILE', 'EIO', 'ENAMETOOLONG', 'ELOOP', 'ENOTSUP', 'EOPNOTSUPP', 'EXDEV', 'EBUSY', 'EINVAL',
])

function systemCode(error: unknown): string | undefined {
  if (!(error instanceof Error)) return undefined
  const code = (error as NodeJS.ErrnoException).code
  return code && FILESYSTEM_ERRORS.has(code) ? code : undefined
}

function beneath(root: string, candidate: string): boolean {
  const path = relative(root, candidate)
  return path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path))
}

class OutputError extends Error {
  constructor(readonly code: 'OUTPUT_INVALID' | 'OUTPUT_EXISTS') {
    super(code)
  }
}

async function checkOutput(source: string, output: string, options: PackOptions): Promise<void> {
  if (!output.endsWith('.uipilot-plugin')) throw new OutputError('OUTPUT_INVALID')
  try {
    await lstat(output)
    throw new OutputError('OUTPUT_EXISTS')
  } catch (error) {
    if (systemCode(error) !== 'ENOENT') throw error
  }
  const parent = dirname(output)
  const probe = (options.hostPlatform ?? process.platform) === 'win32'
    ? options.probe ?? createWindowsAttributeProbe()
    : undefined
  let current = parent
  for (;;) {
    const metadata = await lstat(current)
    if (!metadata.isDirectory() || metadata.isSymbolicLink()) throw new OutputError('OUTPUT_INVALID')
    if (probe) assertNoReparsePoints([''], await probe(current, ['']))
    const next = dirname(current)
    if (next === current) break
    current = next
  }
  const [canonicalSource, canonicalParent] = await Promise.all([realpath(source), realpath(parent)])
  if (beneath(canonicalSource, join(canonicalParent, basename(output)))) {
    throw new OutputError('OUTPUT_INVALID')
  }
}

export async function packPackage(
  request: PackRequest,
  options: PackOptions = {},
): Promise<PluginPackReportV1> {
  const source = resolve(request.source)
  const output = resolve(request.output)
  let validation: PluginValidationReportV1 | undefined
  function failure(code: PluginPackErrorCode, message: string): PluginPackReportV1 {
    return {
      schemaVersion: 1,
      created: false,
      output: { path: output },
      ...(validation ? { validation } : {}),
      error: { code, message },
    }
  }
  let snapshot
  try {
    snapshot = await readDirectorySnapshot(source, options)
  } catch (error) {
    if (error instanceof PackagePolicyError) {
      return failure(
        error.code === 'SOURCE_INVALID' ? 'SOURCE_INVALID' : 'PACKAGE_INVALID',
        'Source package is invalid or unsafe.',
      )
    }
    if (systemCode(error)) return failure('SOURCE_INVALID', 'Source directory is unavailable.')
    throw error
  }
  validation = validateSnapshot(snapshot, source, request.platform)
  if (!validation.valid) return failure('PACKAGE_INVALID', 'Package validation failed.')
  try {
    await checkOutput(source, output, options)
  } catch (error) {
    if (error instanceof OutputError) {
      return failure(error.code, error.code === 'OUTPUT_EXISTS'
        ? 'Output path already exists.' : 'Output path is invalid or unsafe.')
    }
    if (error instanceof PackagePolicyError || systemCode(error)) {
      return failure('OUTPUT_INVALID', 'Output parent must be an existing ordinary directory outside the source.')
    }
    throw error
  }
  let bytes: Buffer
  try {
    bytes = writeArchiveSnapshot(snapshot)
    const archiveValidation = validateSnapshot(readArchiveSnapshot(bytes), source, request.platform)
    if (!archiveValidation.valid) {
      validation = archiveValidation
      return failure('PACKAGE_INVALID', 'Generated archive validation failed.')
    }
  } catch (error) {
    if (error instanceof PackagePolicyError) {
      return failure('PACKAGE_INVALID', 'Package cannot be archived within package limits.')
    }
    throw error
  }
  const io: PublicationOperations = {
    open,
    link,
    unlink,
    write: async (handle, contents) => { await handle.writeFile(contents) },
    ...options.publication,
  }
  const temporary = join(dirname(output), `.uipilot-pack-${randomUUID()}.tmp`)
  let owned = false
  let cleanupFailed = false
  try {
    const handle = await io.open(temporary, 'wx')
    owned = true
    try {
      await io.write(handle, bytes)
    } finally {
      await handle.close()
    }
    try {
      await io.link(temporary, output)
    } catch (error) {
      if (systemCode(error)) {
        // Windows may report EPERM for a directory that raced the hard-link operation.
        const occupied = systemCode(error) === 'EEXIST'
          || await lstat(output).then(() => true, () => false)
        if (occupied) return failure('OUTPUT_EXISTS', 'Output path already exists.')
      }
      throw error
    }
  } catch (error) {
    if (systemCode(error)) return failure('OUTPUT_WRITE_FAILED', 'Output could not be written.')
    throw error
  } finally {
    if (owned) {
      try {
        await io.unlink(temporary)
      } catch {
        cleanupFailed = true
      }
    }
  }
  // Once published, cleanup cannot change the successful result or remove the output.
  return {
    schemaVersion: 1,
    created: true,
    output: { path: output, sha256: createHash('sha256').update(bytes).digest('hex') },
    validation,
    ...(cleanupFailed ? {
      warnings: [{
        code: 'TEMP_CLEANUP_FAILED' as const,
        message: 'Output was created, but its temporary file could not be removed.',
      }],
    } : {}),
  }
}
