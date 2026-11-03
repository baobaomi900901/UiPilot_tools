import { spawn } from 'node:child_process'
import { lstat, realpath } from 'node:fs/promises'
import { win32 } from 'node:path'

import { PackagePolicyError } from './package-policy.js'

export type WindowsAttributeProbe = (
  root: string,
  canonicalRelativePaths: readonly string[],
) => Promise<ReadonlyMap<string, number>>

const MAX_PROTOCOL_BYTES = 256 * 1024
const MAX_PROTOCOL_PATHS = 321
const REPARSE_POINT = 0x400

const SCRIPT = String.raw`
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false, $true)
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
while ($null -ne ($line = [Console]::In.ReadLine())) {
$request = $line | ConvertFrom-Json
$entries = foreach ($relative in @($request.paths)) {
  $nativeRelative = [string]$relative -replace '/', '\'
  $candidate = if ($nativeRelative.Length -eq 0) { [string]$request.root } else { [IO.Path]::Combine([string]$request.root, $nativeRelative) }
  [ordered]@{ path = [string]$relative; attributes = [int64][IO.File]::GetAttributes($candidate) }
}
[Console]::Out.WriteLine((ConvertTo-Json ([ordered]@{ entries = @($entries) }) -Compress -Depth 4))
[Console]::Out.Flush()
}
`

function invalid(message: string): PackagePolicyError {
  return new PackagePolicyError('SOURCE_INVALID', message)
}

function asciiFold(value: string): string {
  return value.replace(/[A-Z]/gu, (character) => character.toLowerCase())
}

function sameWindowsPath(left: string, right: string): boolean {
  return asciiFold(win32.normalize(left)) === asciiFold(win32.normalize(right))
}

function beneath(root: string, candidate: string): boolean {
  const relative = win32.relative(root, candidate)
  return relative === '' || (!relative.startsWith('..') && !win32.isAbsolute(relative))
}

async function verifiedSystemPaths(environment: NodeJS.ProcessEnv): Promise<{
  systemRoot: string
  system32: string
  temp: string
  executable: string
}> {
  const systemRootValue = environment.SystemRoot
  const windirValue = environment.WINDIR
  if (!systemRootValue || !windirValue || !/^[A-Za-z]:\\/u.test(systemRootValue) || !/^[A-Za-z]:\\/u.test(windirValue)) {
    throw invalid('Windows system directory is unavailable.')
  }
  const [systemRoot, windir] = await Promise.all([realpath(systemRootValue), realpath(windirValue)])
  if (!sameWindowsPath(systemRoot, windir)) throw invalid('Windows system directories do not match.')

  const system32Candidate = win32.join(systemRoot, 'System32')
  const tempCandidate = win32.join(systemRoot, 'Temp')
  const executableCandidate = win32.join(system32Candidate, 'WindowsPowerShell', 'v1.0', 'powershell.exe')
  const [system32, temp, executable] = await Promise.all([
    realpath(system32Candidate),
    realpath(tempCandidate),
    realpath(executableCandidate),
  ])
  const [system32Stat, tempStat, executableStat] = await Promise.all([
    lstat(system32),
    lstat(temp),
    lstat(executable),
  ])
  if (
    !system32Stat.isDirectory() ||
    system32Stat.isSymbolicLink() ||
    !tempStat.isDirectory() ||
    tempStat.isSymbolicLink() ||
    !executableStat.isFile() ||
    executableStat.isSymbolicLink() ||
    !sameWindowsPath(system32, system32Candidate) ||
    !sameWindowsPath(temp, tempCandidate) ||
    !sameWindowsPath(executable, executableCandidate) ||
    !beneath(systemRoot, system32) ||
    !beneath(systemRoot, temp) ||
    !beneath(systemRoot, executable)
  ) {
    throw invalid('Windows PowerShell path verification failed.')
  }
  return { systemRoot, system32, temp, executable }
}

function startProbe(
  paths: Awaited<ReturnType<typeof verifiedSystemPaths>>,
): { request: WindowsAttributeProbe; close: () => void } {
  const encodedScript = Buffer.from(SCRIPT, 'utf16le').toString('base64')

  const child = spawn(
    paths.executable,
    ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', encodedScript],
    {
      cwd: paths.system32,
      windowsHide: true,
      env: {
        SystemRoot: paths.systemRoot,
        WINDIR: paths.systemRoot,
        TEMP: paths.temp,
        TMP: paths.temp,
      },
      stdio: ['pipe', 'pipe', 'pipe'],
    },
  )
  let stopped = false
  let output = Buffer.alloc(0)
  let pending: { paths: readonly string[]; resolve: (value: ReadonlyMap<string, number>) => void;
    reject: (error: Error) => void; timeout: ReturnType<typeof setTimeout> } | undefined
  function stop(message = 'Windows attribute probe closed.'): void {
    if (stopped) return
    stopped = true
    const current = pending
    pending = undefined
    if (current) { clearTimeout(current.timeout); current.reject(invalid(message)) }
    child.stdin.destroy()
    child.kill()
  }
  child.on('error', () => stop('Windows attribute probe failed.'))
  child.on('exit', () => stop('Windows attribute probe exited unexpectedly.'))
  child.stdin.on('error', () => stop('Windows attribute probe input failed.'))
  child.stdout.on('error', () => stop('Windows attribute probe output failed.'))
  child.stderr.on('error', () => stop('Windows attribute probe output failed.'))
  child.stderr.on('data', () => stop('Windows attribute probe failed.'))
  child.stdout.on('data', (chunk: Buffer) => {
    if (stopped) return
    if (!pending || output.byteLength + chunk.byteLength > MAX_PROTOCOL_BYTES) {
      stop('Windows attribute response is invalid.')
      return
    }
    output = Buffer.concat([output, chunk])
    const newline = output.indexOf(10)
    if (newline < 0) return
    if (newline !== output.length - 1) { stop('Windows attribute response is invalid.'); return }
    try {
      const response = parseResponse(output.subarray(0, newline), pending.paths)
      const current = pending
      pending = undefined
      output = Buffer.alloc(0)
      clearTimeout(current.timeout)
      current.resolve(response)
    } catch { stop('Windows attribute response is invalid.') }
  })
  return {
    close: stop,
    request: (root, canonicalRelativePaths) => new Promise((resolve, reject) => {
      if (stopped || pending) { reject(invalid('Windows attribute probe is unavailable.')); return }
      if (canonicalRelativePaths.length > MAX_PROTOCOL_PATHS) {
        reject(invalid('Windows attribute request is too large.')); return
      }
      const request = Buffer.from(JSON.stringify({ root, paths: canonicalRelativePaths }) + '\n', 'utf8')
      if (request.byteLength > MAX_PROTOCOL_BYTES) {
        reject(invalid('Windows attribute request is too large.')); return
      }
      pending = { paths: canonicalRelativePaths, resolve, reject,
        timeout: setTimeout(() => stop('Windows attribute probe timed out.'), 5_000) }
      child.stdin.write(request)
    }),
  }
}

function parseResponse(bytes: Buffer, canonicalRelativePaths: readonly string[]): ReadonlyMap<string, number> {
  let response: unknown
  try {
    response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes))
  } catch {
    throw invalid('Windows attribute response is invalid.')
  }
  if (!response || typeof response !== 'object' || !Array.isArray((response as { entries?: unknown }).entries)) {
    throw invalid('Windows attribute response is invalid.')
  }
  const result = new Map<string, number>()
  for (const entry of (response as { entries: unknown[] }).entries) {
    if (
      !entry ||
      typeof entry !== 'object' ||
      typeof (entry as { path?: unknown }).path !== 'string' ||
      typeof (entry as { attributes?: unknown }).attributes !== 'number' ||
      !Number.isSafeInteger((entry as { attributes: number }).attributes)
    ) {
      throw invalid('Windows attribute response is invalid.')
    }
    const path = (entry as { path: string }).path
    if (result.has(path)) throw invalid('Windows attribute response contains duplicates.')
    result.set(path, (entry as { attributes: number }).attributes)
  }
  if (result.size !== canonicalRelativePaths.length || canonicalRelativePaths.some((path) => !result.has(path))) {
    throw invalid('Windows attribute response is incomplete.')
  }
  return result
}

export function createWindowsAttributeProbe(
  environment: NodeJS.ProcessEnv = process.env,
): WindowsAttributeProbe & { close: () => void } {
  const frozenEnvironment = {
    SystemRoot: environment.SystemRoot,
    WINDIR: environment.WINDIR,
  }
  let pathsPromise: ReturnType<typeof verifiedSystemPaths> | undefined
  let session: ReturnType<typeof startProbe> | undefined
  let closed = false
  const probe: WindowsAttributeProbe = async (root, canonicalRelativePaths) => {
    if (closed) throw invalid('Windows attribute probe closed.')
    pathsPromise ??= verifiedSystemPaths(frozenEnvironment)
    const paths = await pathsPromise
    if (closed) throw invalid('Windows attribute probe closed.')
    session ??= startProbe(paths)
    return session.request(root, canonicalRelativePaths)
  }
  return Object.assign(probe, { close: () => { closed = true; session?.close() } })
}

export async function withWindowsAttributeProbe<T>(
  platform: NodeJS.Platform,
  injected: WindowsAttributeProbe | undefined,
  operation: (probe: WindowsAttributeProbe | undefined) => Promise<T>,
): Promise<T> {
  if (platform !== 'win32') return operation(undefined)
  if (injected) return operation(injected)
  const probe = createWindowsAttributeProbe()
  try { return await operation(probe) } finally { probe.close() }
}

export function assertNoReparsePoints(
  paths: readonly string[],
  attributes: ReadonlyMap<string, number>,
): void {
  if (
    paths.some((path) => {
      const value = attributes.get(path)
      return value === undefined || (value & REPARSE_POINT) !== 0
    })
  ) {
    throw invalid('Package contains an unsafe Windows reparse point.')
  }
}
