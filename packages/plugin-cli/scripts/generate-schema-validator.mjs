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
    value.maximum = 4294967295
  } else if (value.format === 'double') {
    delete value.format
  }
  for (const child of Object.values(value)) inlineNumericFormats(child)
}

inlineNumericFormats(schema)
const ajv = new Ajv2020({
  strict: true,
  allErrors: true,
  allowUnionTypes: true,
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
