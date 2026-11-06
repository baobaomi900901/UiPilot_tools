import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { cp, mkdir, mkdtemp, realpath, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = join(packageRoot, '..', '..')
const hostVersion = JSON.parse(await readFile(join(repositoryRoot, 'package.json'), 'utf8')).version
const temporaryRoot = await mkdtemp(join(await realpath(tmpdir()), 'uipilot-plugin-cli-smoke-'))
const packDirectory = join(temporaryRoot, 'pack')
const projectDirectory = join(temporaryRoot, 'consumer')
const cacheDirectory = join(temporaryRoot, 'empty-cache')
const fixturesDirectory = join(temporaryRoot, 'fixtures')
const outputDirectory = join(temporaryRoot, 'packed output')

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

// Pack checks metadata all the way to the filesystem root. Node cannot grant
// metadata-only reads, so pack has broad reads but can write only its output directory.
// Validate retains the narrower read-only sandbox above.
function packWithArtifact(cli, trap, source, output, expectedExitCode = 0) {
  const args = ['--experimental-permission', '--allow-fs-read=*', '--allow-fs-write=' + outputDirectory]
  if (process.platform === 'win32') args.push('--allow-child-process')
  args.push('--import', pathToFileURL(trap).href, cli, 'pack', source, '--output', output, '--platform', 'windows', '--json')
  const result = run(process.execPath, args)
  assert.equal(result.status, expectedExitCode, result.stdout + '\n' + result.stderr)
  const report = JSON.parse(result.stdout)
  assert.equal(report.schemaVersion, 1)
  assert.equal(report.created, expectedExitCode === 0)
  return report
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
    mkdir(outputDirectory, { recursive: true }),
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
  assert.match(binHelp.stdout, /uipilot-plugin pack/u)

  const examples = [
    'com.uipilot.demo-return',
    'com.uipilot.demo-multi',
    'com.uipilot.demo-win',
    'com.uipilot.demo-panel',
    'com.uipilot.demo-credentials',
    'com.uipilot.pomodoro',
  ]
  for (const name of examples) {
    await cp(join(repositoryRoot, 'examples', 'public-plugins', name, 'package'), join(fixturesDirectory, name), {
      recursive: true,
    })
  }
  const trap = join(temporaryRoot, 'network-trap.mjs')
  await cp(join(packageRoot, 'scripts', 'network-trap.mjs'), trap)
  const cli = join(projectDirectory, 'node_modules', '@uipilot', 'plugin-cli', 'dist', 'cli.mjs')

  for (const source of examples.map((name) => join(fixturesDirectory, name))) {
    validateWithArtifact(cli, trap, source, 0)
  }
  for (const name of ['com.uipilot.demo-return', 'com.uipilot.demo-multi', 'com.uipilot.demo-panel', 'com.uipilot.pomodoro', 'com.uipilot.demo-credentials']) {
    const source = join(fixturesDirectory, name)
    const output = join(outputDirectory, name + ' local.uipilot-plugin')
    const packedReport = packWithArtifact(cli, trap, source, output)
    assert.equal(packedReport.output.path, output)
    assert.match(packedReport.output.sha256, /^[a-f0-9]{64}$/u)
    assert.equal(packedReport.validation.valid, true)
    const archiveReport = validateWithArtifact(cli, trap, output, 0)
    const directoryReport = validateWithArtifact(cli, trap, source, 0)
    assert.deepEqual({ ...archiveReport, source: directoryReport.source }, directoryReport)
    assert.deepEqual(packedReport.validation, directoryReport)
    if (name === 'com.uipilot.demo-multi') {
      assert.deepEqual(directoryReport.target, { platform: 'windows', hostVersion, apiVersion: 1 })
      assert.deepEqual(directoryReport.plugin.commands, [
        { commandId: 'upper', defaultName: 'demo-upper', activationMode: 'live', outputMode: 'mainResult' },
        { commandId: 'lower', defaultName: 'demo-lower', activationMode: 'submit', outputMode: 'mainResult' },
      ])
      const bytes = await readFile(output)
      const sha256 = createHash('sha256').update(bytes).digest('hex')
      assert.equal(packedReport.output.sha256, sha256)
      const tracked = await readFile(join(repositoryRoot, 'tests/fixtures/public-plugin-pack/multi-command.uipilot-plugin'))
      assert.deepEqual(bytes, tracked)
      assert.equal(sha256, createHash('sha256').update(tracked).digest('hex'))
      const secondOutput = join(outputDirectory, name + ' repeated.uipilot-plugin')
      const second = packWithArtifact(cli, trap, source, secondOutput)
      assert.deepEqual(await readFile(secondOutput), bytes)
      assert.equal(second.output.sha256, sha256)
      const refused = packWithArtifact(cli, trap, source, output, 1)
      assert.equal(refused.error.code, 'OUTPUT_EXISTS')
      assert.deepEqual(await readFile(output), bytes)
      console.log(`multi-command fixture: fresh installed CLI bytes equal tracked Rust fixture; SHA-256 ${sha256}`)
    }
    if (name === 'com.uipilot.demo-return') {
      const before = await readFile(output)
      const repeated = packWithArtifact(cli, trap, source, output, 1)
      assert.equal(repeated.error.code, 'OUTPUT_EXISTS')
      assert.deepEqual(await readFile(output), before)
    }
  }
  // A one-item commands array remains the new form and must pass the production CLI too.
  const single = join(fixturesDirectory, 'single-command')
  await cp(join(fixturesDirectory, 'com.uipilot.demo-multi'), single, { recursive: true })
  const singleManifest = JSON.parse(await readFile(join(single, 'plugin.json'), 'utf8'))
  singleManifest.commands = singleManifest.commands.slice(0, 1)
  await writeFile(join(single, 'plugin.json'), JSON.stringify(singleManifest) + '\n')
  const singleReport = validateWithArtifact(cli, trap, single, 0)
  assert.equal(singleReport.plugin.commands.length, 1)
  const singleOutput = join(outputDirectory, 'single-command.uipilot-plugin')
  const singlePack = packWithArtifact(cli, trap, single, singleOutput)
  assert.deepEqual(singlePack.validation, singleReport)
  assert.deepEqual({ ...validateWithArtifact(cli, trap, singleOutput, 0), source: singleReport.source }, singleReport)
  validateWithArtifact(cli, trap, join(fixturesDirectory, 'com.uipilot.pomodoro'), 1, 'macos')

  const invalidManifest = join(fixturesDirectory, 'invalid-manifest')
  await cp(join(fixturesDirectory, 'com.uipilot.demo-return'), invalidManifest, { recursive: true })
  const manifest = JSON.parse(await readFile(join(invalidManifest, 'plugin.json'), 'utf8'))
  delete manifest.pluginId
  await writeFile(join(invalidManifest, 'plugin.json'), `${JSON.stringify(manifest)}\n`, 'utf8')
  const manifestReport = validateWithArtifact(cli, trap, invalidManifest, 1)
  assert.ok(manifestReport.issues.some(({ code }) => code === 'MANIFEST_SCHEMA_INVALID'))
  const invalidOutput = join(outputDirectory, 'invalid.uipilot-plugin')
  const failedPack = packWithArtifact(cli, trap, invalidManifest, invalidOutput, 1)
  assert.equal(failedPack.error.code, 'PACKAGE_INVALID')
  await assert.rejects(readFile(invalidOutput), { code: 'ENOENT' })

  const invalidTimer = join(fixturesDirectory, 'invalid-timer')
  await cp(join(fixturesDirectory, 'com.uipilot.pomodoro'), invalidTimer, { recursive: true })
  await writeFile(join(invalidTimer, 'assets', 'sounds', 'timer-alarm.wav'), 'not a wave')
  const timerReport = validateWithArtifact(cli, trap, invalidTimer, 1)
  assert.ok(timerReport.issues.some(({ code }) => code === 'RESOURCE_INVALID'))
} finally {
  await rm(temporaryRoot, { recursive: true, force: true })
}
