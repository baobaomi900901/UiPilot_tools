import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import Ajv2020 from 'ajv/dist/2020.js'
import standaloneCode from 'ajv/dist/standalone/index.js'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const schemaPath = join(packageRoot, 'schema', 'uipilot-plugin-v1.schema.json')
const outputPath = join(packageRoot, 'src', 'generated', 'manifest-validator.mjs')
const schema = JSON.parse(await readFile(schemaPath, 'utf8'))

function inlineNumericFormats(value) {
  if (Array.isArray(value)) {
    for (const item of value) inlineNumericFormats(item)
    return
  }
  if (!value || typeof value !== 'object') return
  if (value.format === 'uint32') {
    delete value.format
    value.maximum = Math.min(value.maximum ?? 4294967295, 4294967295)
  } else if (value.format === 'double') {
    delete value.format
  }
  for (const child of Object.values(value)) inlineNumericFormats(child)
}

function inlinePanelHostKeyDeclaration(value) {
  const declaration = value?.$defs?.PanelHostKeyDeclaration
  const items = value?.$defs?.PublicPanelV1?.properties?.hostKeys?.items
  if (!declaration || items?.$ref !== '#/$defs/PanelHostKeyDeclaration') {
    throw new Error('panel host key Schema shape is invalid')
  }
  value.$defs.PublicPanelV1.properties.hostKeys.items = structuredClone(declaration)
}

function inlineShareReceiverConstraints(value) {
  const properties = value?.$defs?.PublicShareReceiverV1?.properties
  const title = properties?.title
  const types = properties?.types
  if (title?.type !== 'string' || title.minLength !== 1 || title.maxLength !== 80 ||
      typeof title.pattern !== 'string' || JSON.stringify(types) !== '{"const":["text/plain"]}') {
    throw new Error('share receiver Schema shape is invalid')
  }
  // Ajv emits runtime helpers for Unicode length and object/array const equality.
  // Its Unicode regexp mode counts code points, including astral pairs; the
  // negative lookahead anchors the absolute end even for a final line break.
  delete title.minLength
  delete title.maxLength
  title.allOf = [{ pattern: '^[\\s\\S]{1,80}(?![\\s\\S])' }]
  properties.types = { type: 'array', minItems: 1, maxItems: 1, items: { const: 'text/plain' } }
}

inlineNumericFormats(schema)
inlinePanelHostKeyDeclaration(schema)
inlineShareReceiverConstraints(schema)
const ajv = new Ajv2020({
  strict: true,
  allErrors: true,
  allowUnionTypes: true,
  unicodeRegExp: true,
  code: { esm: true, source: true, lines: true },
})
const validate = ajv.compile(schema)
const generated = `${standaloneCode(ajv, validate)}\n`

if (process.argv.includes('--check')) {
  const output = await readFile(outputPath, 'utf8').catch(() => '')
  if (output !== generated) throw new Error('generated Manifest validator is stale')
} else {
  await mkdir(dirname(outputPath), { recursive: true })
  await writeFile(outputPath, generated, 'utf8')
}
