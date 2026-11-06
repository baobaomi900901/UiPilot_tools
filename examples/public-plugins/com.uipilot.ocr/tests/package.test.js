import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile, readdir, stat } from 'node:fs/promises'
const root = new URL('../package/', import.meta.url)
test('all declared assets, HTML references and relative module imports remain inside the strict package', async () => {
 const manifest = JSON.parse(await readFile(new URL('plugin.json', root), 'utf8'))
 assert.deepEqual((await readdir(root)).sort(), ['dist', 'plugin.json'])
 assert.deepEqual((await readdir(new URL('dist/', root))).sort(), ['panel-model.js', 'panel.css', 'panel.html', 'panel.js', 'providers.js', 'runtime.js'])
 for (const entry of [manifest.runtime.entry, manifest.panel.entry]) assert.equal((await stat(new URL(entry, root))).isFile(), true)
 for (const name of await readdir(new URL('dist/', root))) {
  const source = await readFile(new URL(`dist/${name}`, root), 'utf8')
  for (const match of source.matchAll(/(?:from\s+|(?:src|href)=)["'](\.\/?[^"']+)["']/g)) {
   const target = new URL(match[1], new URL(`dist/${name}`, root)); assert.ok(target.href.startsWith(root.href)); assert.equal((await stat(target)).isFile(), true)
  }
  assert.doesNotMatch(source, /(?:fetch\(|localStorage|sessionStorage|indexedDB|navigator\.clipboard|__TAURI|invoke\()/)
 }
 await assert.rejects(stat(new URL('../../com.uipilot.baidu-ocr/', root)), { code: 'ENOENT' })
 const css = await readFile(new URL('dist/panel.css', root), 'utf8')
 assert.match(css, /data-theme='light'/); assert.match(css, /data-theme='dark'/); assert.match(css, /forced-colors/)
 assert.match(css, /object-fit: contain/); assert.match(css, /grid-template-columns: minmax\(0, 1fr\) minmax\(0, 1fr\)/)
 const html = await readFile(new URL('dist/panel.html', root), 'utf8')
 assert.match(html, /<textarea/); assert.doesNotMatch(html, /maxlength=|autofocus/)
})
