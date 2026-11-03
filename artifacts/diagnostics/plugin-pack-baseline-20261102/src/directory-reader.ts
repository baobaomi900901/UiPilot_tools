import { lstat, open, readdir } from 'node:fs/promises'
import { join } from 'node:path'

import { PackagePolicyError, SnapshotBuilder, type PackageSnapshot } from './package-policy.js'
import {
  assertNoReparsePoints,
  createWindowsAttributeProbe,
  type WindowsAttributeProbe,
} from './windows-attributes.js'

export interface DirectoryReaderOptions {
  hostPlatform?: NodeJS.Platform
  probe?: WindowsAttributeProbe
}

function invalid(): PackagePolicyError {
  return new PackagePolicyError('SOURCE_INVALID', 'Source directory changed or is unsafe.')
}

function sameIdentity(
  left: Awaited<ReturnType<typeof lstat>>,
  right: Awaited<ReturnType<typeof lstat>>,
): boolean {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeMs === right.mtimeMs &&
    left.isFile() === right.isFile() &&
    left.isDirectory() === right.isDirectory()
  )
}

async function safeAttributes(
  root: string,
  relativePath: string,
  probe: WindowsAttributeProbe | undefined,
): Promise<void> {
  if (!probe) return
  const paths = [relativePath]
  assertNoReparsePoints(paths, await probe(root, paths))
}

export async function readDirectorySnapshot(
  root: string,
  options: DirectoryReaderOptions = {},
): Promise<PackageSnapshot> {
  const platform = options.hostPlatform ?? process.platform
  const probe = platform === 'win32' ? options.probe ?? createWindowsAttributeProbe() : undefined
  const rootBefore = await lstat(root).catch(() => {
    throw invalid()
  })
  if (!rootBefore.isDirectory() || rootBefore.isSymbolicLink()) throw invalid()
  await safeAttributes(root, '', probe)
  const builder = new SnapshotBuilder('directory')

  async function walk(directory: string, relativeDirectory: string): Promise<void> {
    const before = await lstat(directory).catch(() => {
      throw invalid()
    })
    if (!before.isDirectory() || before.isSymbolicLink()) throw invalid()
    await safeAttributes(root, relativeDirectory, probe)
    const entries = await readdir(directory, { withFileTypes: true }).catch(() => {
      throw invalid()
    })
    entries.sort((left, right) => Buffer.compare(Buffer.from(left.name), Buffer.from(right.name)))

    for (const entry of entries) {
      const relativePath = relativeDirectory ? `${relativeDirectory}/${entry.name}` : entry.name
      const absolutePath = join(directory, entry.name)
      const pathBefore = await lstat(absolutePath).catch(() => {
        throw invalid()
      })
      if (pathBefore.isSymbolicLink()) throw invalid()
      await safeAttributes(root, relativePath, probe)
      if (pathBefore.isDirectory()) {
        builder.addDirectory(relativePath)
        await walk(absolutePath, relativePath)
      } else if (pathBefore.isFile()) {
        const handle = await open(absolutePath, 'r').catch(() => {
          throw invalid()
        })
        try {
          const handleBefore = await handle.stat()
          if (!sameIdentity(pathBefore, handleBefore)) throw invalid()
          const bytes = await handle.readFile()
          const [handleAfter, pathAfter] = await Promise.all([handle.stat(), lstat(absolutePath)])
          if (!sameIdentity(handleBefore, handleAfter) || !sameIdentity(pathBefore, pathAfter)) throw invalid()
          builder.addFile(relativePath, bytes)
        } finally {
          await handle.close()
        }
      } else {
        throw invalid()
      }
      await safeAttributes(root, relativePath, probe)
    }
    const after = await lstat(directory).catch(() => {
      throw invalid()
    })
    if (!sameIdentity(before, after)) throw invalid()
    await safeAttributes(root, relativeDirectory, probe)
  }

  await walk(root, '')
  const rootAfter = await lstat(root).catch(() => {
    throw invalid()
  })
  if (!sameIdentity(rootBefore, rootAfter)) throw invalid()
  return builder.finish()
}
