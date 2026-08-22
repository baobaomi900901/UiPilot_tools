import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const sourcePath = join(packageRoot, '..', '..', 'docs', 'plugin-sdk', 'uipilot-plugin-v1.schema.json')
const outputPath = join(packageRoot, 'schema', 'uipilot-plugin-v1.schema.json')
const source = await readFile(sourcePath, 'utf8')

if (process.argv.includes('--check')) {
  const output = await readFile(outputPath, 'utf8').catch(() => '')
  if (output !== source) throw new Error('bundled plugin Schema is stale')
} else {
  await mkdir(dirname(outputPath), { recursive: true })
  await writeFile(outputPath, source, 'utf8')
}
