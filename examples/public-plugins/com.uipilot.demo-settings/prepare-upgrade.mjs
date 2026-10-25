import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'

// Explicit output only: creates a compatible package, never installs or touches Host data.
const destination = process.argv[2]
if (!destination) throw new Error('Usage: node prepare-upgrade.mjs <empty-output-directory>')
const target = resolve(destination)
await mkdir(target, { recursive: true })
const source = new URL('./package/', import.meta.url)
const manifest = JSON.parse((await readFile(new URL('plugin.json', source), 'utf8')).replace(/^\uFEFF/, ''))
manifest.version = '1.0.1'
await writeFile(join(target, 'plugin.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx' })
const runtime = await readFile(new URL('runtime.js', source), 'utf8')
await writeFile(join(target, 'runtime.js'), runtime.replace("const BUILD = '1.0.0'", "const BUILD = '1.0.1'"), { flag: 'wx' })
console.log(target)
