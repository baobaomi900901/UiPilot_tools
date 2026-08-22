import { foldName, normalizeNfc } from './unicode.js'

export type PackagePolicyErrorCode =
  | 'SOURCE_INVALID'
  | 'ARCHIVE_INVALID'
  | 'PACKAGE_LIMIT_EXCEEDED'
  | 'PATH_INVALID'
  | 'PATH_COLLISION'
  | 'RESOURCE_INVALID'

export class PackagePolicyError extends Error {
  constructor(
    readonly code: PackagePolicyErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'PackagePolicyError'
  }
}

export interface PackageSnapshot {
  readonly kind: 'directory' | 'archive'
  readonly files: ReadonlyMap<string, Buffer>
  readonly directories: readonly string[]
  readonly totalBytes: number
}

export const PACKAGE_LIMITS = {
  maxDirectories: 64,
  maxFiles: 256,
  maxDepth: 8,
  maxFileBytes: 2 * 1024 * 1024,
  maxTotalBytes: 16 * 1024 * 1024,
  maxPathBytes: 240,
  maxComponentBytes: 100,
  maxArchiveBytes: 16 * 1024 * 1024,
  maxArchiveEntries: 320,
} as const

const windowsReserved = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])$/iu
const forbidden = /[<>:"|?*]/u

function validateResource(path: string): void {
  if (path === 'plugin.json' || path === 'assets/sounds/timer-alarm.wav') return
  const basename = path.slice(path.lastIndexOf('/') + 1)
  const parts = basename.split('.')
  if (parts.length !== 2 || !parts[0]) {
    throw new PackagePolicyError('RESOURCE_INVALID', `Unsupported package resource: ${path}`)
  }
  const extension = parts[1]
  if (!['html', 'js', 'css', 'png'].includes(extension)) {
    throw new PackagePolicyError('RESOURCE_INVALID', `Unsupported package resource: ${path}`)
  }
  if (extension === 'png' && path !== 'icon.png') {
    throw new PackagePolicyError('RESOURCE_INVALID', 'icon.png is the only allowed PNG resource.')
  }
}

export function canonicalRelativePath(value: string): string {
  if (
    value.length === 0 ||
    value.startsWith('/') ||
    value.includes('\\') ||
    Buffer.byteLength(value, 'utf8') > PACKAGE_LIMITS.maxPathBytes
  ) {
    throw new PackagePolicyError('PATH_INVALID', `Invalid package path: ${value}`)
  }
  const components = value.split('/')
  if (components.length > PACKAGE_LIMITS.maxDepth) {
    throw new PackagePolicyError('PATH_INVALID', `Package path is too deep: ${value}`)
  }
  for (const component of components) {
    const stem = component.split('.', 1)[0]
    if (
      component.length === 0 ||
      component === '.' ||
      component === '..' ||
      Buffer.byteLength(component, 'utf8') > PACKAGE_LIMITS.maxComponentBytes ||
      component.endsWith('.') ||
      component.endsWith(' ') ||
      forbidden.test(component) ||
      /\p{Cc}/u.test(component) ||
      windowsReserved.test(stem) ||
      normalizeNfc(component) !== component
    ) {
      throw new PackagePolicyError('PATH_INVALID', `Invalid package path: ${value}`)
    }
  }
  const canonical = components.join('/')
  validateResource(canonical)
  return canonical
}

export class SnapshotBuilder {
  readonly #files = new Map<string, Buffer>()
  readonly #fileIdentities = new Map<string, string>()
  readonly #directories = new Map<string, string>()
  #totalBytes = 0

  constructor(readonly kind: 'directory' | 'archive') {}

  addDirectory(rawPath: string): void {
    const path = rawPath.endsWith('/') ? rawPath.slice(0, -1) : rawPath
    if (!path) return
    const canonical = canonicalRelativePath(`${path}/placeholder.js`).slice(0, -'/placeholder.js'.length)
    this.#registerDirectory(canonical)
  }

  addFile(rawPath: string, bytes: Uint8Array): void {
    const path = canonicalRelativePath(rawPath)
    if (bytes.byteLength > PACKAGE_LIMITS.maxFileBytes) {
      throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', `File is too large: ${path}`)
    }
    if (this.#files.size >= PACKAGE_LIMITS.maxFiles) {
      throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', 'Package contains too many files.')
    }
    const identity = foldName(path)
    if (this.#fileIdentities.has(identity) || this.#directories.has(identity)) {
      throw new PackagePolicyError('PATH_COLLISION', `Package path collides: ${path}`)
    }
    const components = path.split('/')
    for (let end = 1; end < components.length; end += 1) {
      this.#registerDirectory(components.slice(0, end).join('/'))
    }
    const nextTotal = this.#totalBytes + bytes.byteLength
    if (nextTotal > PACKAGE_LIMITS.maxTotalBytes) {
      throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', 'Package content is too large.')
    }
    this.#fileIdentities.set(identity, path)
    this.#files.set(path, Buffer.from(bytes))
    this.#totalBytes = nextTotal
  }

  #registerDirectory(path: string): void {
    const identity = foldName(path)
    const existing = this.#directories.get(identity)
    if ((existing && existing !== path) || this.#fileIdentities.has(identity)) {
      throw new PackagePolicyError('PATH_COLLISION', `Package path collides: ${path}`)
    }
    if (!existing) {
      if (this.#directories.size >= PACKAGE_LIMITS.maxDirectories) {
        throw new PackagePolicyError('PACKAGE_LIMIT_EXCEEDED', 'Package contains too many directories.')
      }
      this.#directories.set(identity, path)
    }
  }

  finish(): PackageSnapshot {
    return {
      kind: this.kind,
      files: new Map([...this.#files].sort(([left], [right]) => Buffer.compare(Buffer.from(left), Buffer.from(right)))),
      directories: [...this.#directories.values()].sort((left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right))),
      totalBytes: this.#totalBytes,
    }
  }
}
