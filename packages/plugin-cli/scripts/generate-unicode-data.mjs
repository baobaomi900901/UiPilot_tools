import { createHash } from 'node:crypto'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const dataRoot = join(packageRoot, 'unicode-data')
const outputPath = join(packageRoot, 'src', 'generated', 'unicode-data.ts')
const expectedHashes = new Map([
  ['UnicodeData.txt', '2fc713e6a31a87c4850a37fe2caffa4218180fadb5de86b43a143ddb4581fb86'],
  ['CaseFolding.txt', '4e55acfdc32825a22e87670e9056a3bf94ad7c5400065778e9e10f8314372bcf'],
  ['CompositionExclusions.txt', '59d2d9e3dfdf0a999cf9dae11d594f053631222679a2f5710315ea07f7fe82af'],
])

async function pinnedText(name) {
  const bytes = await readFile(join(dataRoot, name))
  const actual = createHash('sha256').update(bytes).digest('hex')
  if (actual !== expectedHashes.get(name)) throw new Error(`${name} checksum mismatch`)
  return bytes.toString('utf8')
}

function dataLines(text) {
  return text
    .split(/\r?\n/u)
    .map((line) => line.split('#', 1)[0].trim())
    .filter(Boolean)
}

const unicodeData = await pinnedText('UnicodeData.txt')
const caseFolding = await pinnedText('CaseFolding.txt')
const exclusions = new Set(
  dataLines(await pinnedText('CompositionExclusions.txt')).map((line) => Number.parseInt(line, 16)),
)

const decompositions = []
const combiningClasses = []
const compositions = []
for (const line of dataLines(unicodeData)) {
  const fields = line.split(';')
  const codePoint = Number.parseInt(fields[0], 16)
  const combiningClass = Number.parseInt(fields[3], 10)
  if (combiningClass !== 0) combiningClasses.push([codePoint, combiningClass])
  const rawDecomposition = fields[5]
  if (!rawDecomposition || rawDecomposition.startsWith('<')) continue
  const mapping = rawDecomposition.split(' ').map((value) => Number.parseInt(value, 16))
  decompositions.push([codePoint, mapping])
  if (mapping.length === 2 && !exclusions.has(codePoint)) {
    compositions.push([mapping[0], mapping[1], codePoint])
  }
}

const folds = []
for (const line of dataLines(caseFolding)) {
  const [rawCodePoint, status, rawMapping] = line.split(';').map((value) => value.trim())
  if (status !== 'C' && status !== 'F') continue
  folds.push([
    Number.parseInt(rawCodePoint, 16),
    rawMapping.split(' ').map((value) => Number.parseInt(value, 16)),
  ])
}

const generated = `// Generated from Unicode 15.1.0 data. Do not edit.\n` +
  `export const CANONICAL_DECOMPOSITIONS: ReadonlyArray<readonly [number, readonly number[]]> = ${JSON.stringify(decompositions)}\n` +
  `export const CANONICAL_COMBINING_CLASSES: ReadonlyArray<readonly [number, number]> = ${JSON.stringify(combiningClasses)}\n` +
  `export const CANONICAL_COMPOSITIONS: ReadonlyArray<readonly [number, number, number]> = ${JSON.stringify(compositions)}\n` +
  `export const FULL_CASE_FOLDS: ReadonlyArray<readonly [number, readonly number[]]> = ${JSON.stringify(folds)}\n`

if (process.argv.includes('--check')) {
  const existing = await readFile(outputPath, 'utf8').catch(() => '')
  if (existing !== generated) throw new Error('generated Unicode data is stale')
} else {
  await mkdir(dirname(outputPath), { recursive: true })
  await writeFile(outputPath, generated, 'utf8')
}
