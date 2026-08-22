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
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false, $true)
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
$entries = foreach ($relative in @($request.paths)) {
  $nativeRelative = [string]$relative -replace '/', '\'
  $candidate = if ($nativeRelative.Length -eq 0) { [string]$request.root } else { [IO.Path]::Combine([string]$request.root, $nativeRelative) }
  [ordered]@{ path = [string]$relative; attributes = [int64][IO.File]::GetAttributes($candidate) }
}
[Console]::Out.Write((ConvertTo-Json ([ordered]@{ entries = @($entries) }) -Compress -Depth 4))
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

async function runProbe(
  paths: Awaited<ReturnType<typeof verifiedSystemPaths>>,
  root: string,
  canonicalRelativePaths: readonly string[],
): Promise<ReadonlyMap<string, number>> {
  if (canonicalRelativePaths.length > MAX_PROTOCOL_PATHS) throw invalid('Windows attribute request is too large.')
  const request = Buffer.from(JSON.stringify({ root, paths: canonicalRelativePaths }), 'utf8')
  if (request.byteLength > MAX_PROTOCOL_BYTES) throw invalid('Windows attribute request is too large.')
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
  const stdout: Buffer[] = []
  const stderr: Buffer[] = []
  let outputBytes = 0
  child.stdout.on('data', (chunk: Buffer) => {
    outputBytes += chunk.byteLength
    if (outputBytes <= MAX_PROTOCOL_BYTES) stdout.push(chunk)
  })
  child.stderr.on('data', (chunk: Buffer) => stderr.push(chunk))
  child.stdin.end(request)

  const exitCode = await new Promise<number | null>((resolve, reject) => {
    const timeout = setTimeout(() => {
      child.kill()
      reject(invalid('Windows attribute probe timed out.'))
    }, 5_000)
    child.once('error', (error) => {
      clearTimeout(timeout)
      reject(error)
    })
    child.once('exit', (code) => {
      clearTimeout(timeout)
      resolve(code)
    })
  }).catch(() => {
    throw invalid('Windows attribute probe failed.')
  })
  if (exitCode !== 0 || stderr.length > 0 || outputBytes > MAX_PROTOCOL_BYTES) {
    throw invalid('Windows attribute probe failed.')
  }

  let response: unknown
  try {
    response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(stdout)))
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
): WindowsAttributeProbe {
  const frozenEnvironment = {
    SystemRoot: environment.SystemRoot,
    WINDIR: environment.WINDIR,
  }
  let pathsPromise: ReturnType<typeof verifiedSystemPaths> | undefined
  return async (root, canonicalRelativePaths) => {
    pathsPromise ??= verifiedSystemPaths(frozenEnvironment)
    return runProbe(await pathsPromise, root, canonicalRelativePaths)
  }
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
