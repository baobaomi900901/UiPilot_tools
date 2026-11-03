import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile, readdir, access } from 'node:fs/promises'
import { LANGUAGES, PROVIDERS } from '../package/dist/languages.js'
import { getProvider } from '../package/dist/providers.js'
const root = new URL('../package/', import.meta.url)
const manifest = JSON.parse(await readFile(new URL('plugin.json', root), 'utf8'))

test('1.4.0 keeps legacy identity and explicitly requires the released Panel capability floor', () => {
  assert.equal(manifest.pluginId, 'com.uipilot.translate')
  assert.equal(manifest.version, '1.4.0')
  assert.equal(manifest.minimumHostVersion, '0.19.0')
  assert.equal(manifest.command.defaultName, 'translate')
  assert.equal(manifest.commands, undefined)
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.panel, { entry: 'dist/panel.html', runtimeActions: true })
  assert.equal(manifest.runtime.resultActions, undefined)
  assert.deepEqual(manifest.permissions, ['selection.read', 'clipboard.write', 'network.https', 'credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek', 'ui.panel', 'speech.speak'])
  assert.deepEqual(manifest.network.httpsHosts, ['openapi.youdao.com'])
})

test('package contains all declared entries and local import/assets, with only allowed asset types', async () => {
  const files = await readdir(root, { recursive: true, withFileTypes: true })
  for (const file of files.filter(file => file.isFile())) assert.match(file.name, /\.(json|js|html|css|png)$/)
  const pending = [manifest.runtime.entry, manifest.panel?.entry ?? 'dist/panel.html', 'icon.png']
  const seen = new Set()
  while (pending.length) {
    const path = pending.pop()
    if (seen.has(path)) continue
    seen.add(path)
    const url = new URL(path, root)
    await access(url)
    if (!/\.(js|html)$/.test(path)) continue
    const text = await readFile(url, 'utf8')
    const refs = [...text.matchAll(/(?:from\s+['"]|(?:src|href)=["'])(\.?[^'"\s]+)['"]/g)].map(match => match[1])
    for (const ref of refs) {
      assert.ok(!/^(https?:|\/)/.test(ref), 'assets remain package-local')
      pending.push(new URL(ref, url).href.slice(root.href.length))
    }
  }
  for (const path of ['dist/runtime.js', 'dist/panel.html', 'dist/panel.js', 'dist/panel.css', 'dist/panel-model.js', 'dist/panel-capabilities.js', 'dist/languages.js', 'dist/providers.js']) assert.ok(seen.has(path), path)
})

test('language menu and service registry expose only implemented Host languages and provider', () => {
  assert.equal(LANGUAGES.length, 112)
  assert.ok(LANGUAGES.some(item => item.id === 'ja'))
  assert.deepEqual(PROVIDERS.map(item => item.id), ['youdao'])
  for (const provider of PROVIDERS) assert.equal(typeof getProvider(provider.id), 'function')
  assert.equal(getProvider('deepseek'), undefined)
})

test('fresh production CLI rejects an archive candidate missing its declared Panel entry', async t => {
  const { cp, mkdtemp, rm } = await import('node:fs/promises')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { fileURLToPath } = await import('node:url')
  const { spawnSync } = await import('node:child_process')
  const fixture = await mkdtemp(join(tmpdir(), 'translate-package-missing-'))
  t.after(() => rm(fixture, { recursive: true, force: true }))
  await cp(root, fixture, { recursive: true })
  await rm(join(fixture, manifest.panel.entry))
  const cli = fileURLToPath(new URL('../../../../packages/plugin-cli/dist/cli.mjs', import.meta.url))
  const result = spawnSync(process.execPath, [cli, 'validate', fixture, '--platform', 'windows'], { encoding: 'utf8' })
  assert.equal(result.error, undefined)
  assert.notEqual(result.status, 0)
  assert.match(result.stdout + result.stderr, /panel\.html|Panel|entry/i)
})
