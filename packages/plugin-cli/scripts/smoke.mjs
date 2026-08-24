import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = join(packageRoot, '..', '..')
const temporaryRoot = await mkdtemp(join(tmpdir(), 'uipilot-plugin-cli-smoke-'))
const packDirectory = join(temporaryRoot, 'pack')
const projectDirectory = join(temporaryRoot, 'consumer')
const cacheDirectory = join(temporaryRoot, 'empty-cache')
const fixturesDirectory = join(temporaryRoot, 'fixtures')

function npmInvocation(args) {
  const npmCli = process.env.npm_execpath
    ?? (process.platform === 'win32' ? join(dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js') : undefined)
  return npmCli ? [process.execPath, [npmCli, ...args]] : ['npm', args]
}

function isolatedEnvironment() {
  return {
    ...process.env,
    NODE_PATH: '',
    npm_config_audit: 'false',
    npm_config_cache: cacheDirectory,
    npm_config_fund: 'false',
    npm_config_offline: 'true',
    npm_config_proxy: 'http://127.0.0.1:9',
    npm_config_https_proxy: 'http://127.0.0.1:9',
    npm_config_registry: 'http://127.0.0.1:9',
    npm_config_userconfig: join(temporaryRoot, '.npmrc'),
  }
}

function run(executable, args, options = {}) {
  const result = spawnSync(executable, args, {
    cwd: options.cwd ?? projectDirectory,
    encoding: 'utf8',
    env: options.env ?? isolatedEnvironment(),
    shell: options.shell ?? false,
    timeout: 60_000,
  })
  if (result.error) throw result.error
  return result
}

const crcTable = new Uint32Array(256)
for (let index = 0; index < crcTable.length; index += 1) {
  let value = index
  for (let bit = 0; bit < 8; bit += 1) value = (value & 1) === 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
  crcTable[index] = value >>> 0
}

function crc32(bytes) {
  let value = 0xffffffff
  for (const byte of bytes) value = crcTable[(value ^ byte) & 0xff] ^ (value >>> 8)
  return (value ^ 0xffffffff) >>> 0
}

async function listFiles(root, current = root) {
  const files = []
  for (const entry of await readdir(current, { withFileTypes: true })) {
    const absolute = join(current, entry.name)
    if (entry.isDirectory()) files.push(...(await listFiles(root, absolute)))
    else if (entry.isFile()) files.push(relative(root, absolute).replaceAll('\\', '/'))
    else throw new Error(`Smoke fixture contains a non-ordinary entry: ${absolute}`)
  }
  return files.sort()
}

async function createStoredZip(root) {
  const localParts = []
  const centralParts = []
  let localOffset = 0
  for (const file of await listFiles(root)) {
    const name = Buffer.from(file, 'utf8')
    const data = await readFile(join(root, ...file.split('/')))
    const checksum = crc32(data)
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50, 0)
    local.writeUInt16LE(20, 4)
    local.writeUInt16LE(0x800, 6)
    local.writeUInt32LE(checksum, 14)
    local.writeUInt32LE(data.length, 18)
    local.writeUInt32LE(data.length, 22)
    local.writeUInt16LE(name.length, 26)
    const central = Buffer.alloc(46)
    central.writeUInt32LE(0x02014b50, 0)
    central.writeUInt16LE(20, 4)
    central.writeUInt16LE(20, 6)
    central.writeUInt16LE(0x800, 8)
    central.writeUInt32LE(checksum, 16)
    central.writeUInt32LE(data.length, 20)
    central.writeUInt32LE(data.length, 24)
    central.writeUInt16LE(name.length, 28)
    central.writeUInt32LE(localOffset, 42)
    localParts.push(local, name, data)
    centralParts.push(central, name)
    localOffset += local.length + name.length + data.length
  }
  const central = Buffer.concat(centralParts)
  const end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50, 0)
  end.writeUInt16LE(centralParts.length / 2, 8)
  end.writeUInt16LE(centralParts.length / 2, 10)
  end.writeUInt32LE(central.length, 12)
  end.writeUInt32LE(localOffset, 16)
  return Buffer.concat([...localParts, central, end])
}

function cliArguments(cli, trap, source, platform = 'windows') {
  const args = [
    '--experimental-permission',
    `--allow-fs-read=${temporaryRoot}`,
  ]
  if (process.platform === 'win32') {
    args.push(`--allow-fs-read=${process.env.SystemRoot ?? 'C:\\Windows'}`, '--allow-child-process')
  }
  args.push('--import', pathToFileURL(trap).href, cli, 'validate', source, '--platform', platform, '--json')
  return args
}

function validateWithArtifact(cli, trap, source, expectedExitCode, platform = 'windows') {
  const result = run(process.execPath, cliArguments(cli, trap, source, platform))
  assert.equal(result.status, expectedExitCode, `${result.stdout}\n${result.stderr}`)
  const report = JSON.parse(result.stdout)
  assert.equal(report.schemaVersion, 1)
  assert.equal(report.valid, expectedExitCode === 0)
  return report
}

try {
  await Promise.all([
    mkdir(packDirectory, { recursive: true }),
    mkdir(projectDirectory, { recursive: true }),
    mkdir(cacheDirectory, { recursive: true }),
    mkdir(fixturesDirectory, { recursive: true }),
    writeFile(join(temporaryRoot, '.npmrc'), '', 'utf8'),
  ])
  await writeFile(join(projectDirectory, 'package.json'), '{"private":true}\n', 'utf8')

  const [packExecutable, packArguments] = npmInvocation(['pack', '--json', '--pack-destination', packDirectory])
  const pack = run(packExecutable, packArguments, {
    cwd: packageRoot,
  })
  assert.equal(pack.status, 0, `${pack.stdout}\n${pack.stderr}`)
  const packed = JSON.parse(pack.stdout)[0]
  const expectedFiles = [
    'LICENSE',
    'README.md',
    'THIRD_PARTY_NOTICES',
    'dist/cli.mjs',
    'dist/metafile.json',
    'dist/uipilot-plugin-v1.schema.json',
    'package.json',
  ]
  assert.deepEqual(packed.files.map(({ path }) => path).sort(), expectedFiles)
  const tarball = join(packDirectory, packed.filename)

  const [installExecutable, installArguments] = npmInvocation([
    'install',
    '--ignore-scripts',
    '--offline',
    '--no-audit',
    '--no-fund',
    tarball,
  ])
  const installation = run(installExecutable, installArguments)
  assert.equal(installation.status, 0, `${installation.stdout}\n${installation.stderr}`)
  const bin = join(projectDirectory, 'node_modules', '.bin', process.platform === 'win32' ? 'uipilot-plugin.cmd' : 'uipilot-plugin')
  const binHelp = run(bin, ['--help'], { shell: process.platform === 'win32' })
  assert.equal(binHelp.status, 0, `${binHelp.stdout}\n${binHelp.stderr}`)
  assert.match(binHelp.stdout, /Usage: uipilot-plugin/u)

  const examples = ['com.uipilot.demo-return', 'com.uipilot.demo-win', 'com.uipilot.pomodoro']
  for (const name of examples) {
    await cp(join(repositoryRoot, 'examples', 'public-plugins', name, 'package'), join(fixturesDirectory, name), {
      recursive: true,
    })
  }
  const archive = join(fixturesDirectory, 'demo-return.uipilot-plugin')
  await writeFile(archive, await createStoredZip(join(fixturesDirectory, 'com.uipilot.demo-return')))
  const trap = join(temporaryRoot, 'network-trap.mjs')
  await cp(join(packageRoot, 'scripts', 'network-trap.mjs'), trap)
  const cli = join(projectDirectory, 'node_modules', '@uipilot', 'plugin-cli', 'dist', 'cli.mjs')

  for (const source of [...examples.map((name) => join(fixturesDirectory, name)), archive]) {
    validateWithArtifact(cli, trap, source, 0)
  }
  validateWithArtifact(cli, trap, join(fixturesDirectory, 'com.uipilot.pomodoro'), 1, 'macos')

  const invalidManifest = join(fixturesDirectory, 'invalid-manifest')
  await cp(join(fixturesDirectory, 'com.uipilot.demo-return'), invalidManifest, { recursive: true })
  const manifest = JSON.parse(await readFile(join(invalidManifest, 'plugin.json'), 'utf8'))
  delete manifest.pluginId
  await writeFile(join(invalidManifest, 'plugin.json'), `${JSON.stringify(manifest)}\n`, 'utf8')
  const manifestReport = validateWithArtifact(cli, trap, invalidManifest, 1)
  assert.ok(manifestReport.issues.some(({ code }) => code === 'MANIFEST_SCHEMA_INVALID'))

  const invalidTimer = join(fixturesDirectory, 'invalid-timer')
  await cp(join(fixturesDirectory, 'com.uipilot.pomodoro'), invalidTimer, { recursive: true })
  await writeFile(join(invalidTimer, 'assets', 'sounds', 'timer-alarm.wav'), 'not a wave')
  const timerReport = validateWithArtifact(cli, trap, invalidTimer, 1)
  assert.ok(timerReport.issues.some(({ code }) => code === 'RESOURCE_INVALID'))
} finally {
  await rm(temporaryRoot, { recursive: true, force: true })
}
