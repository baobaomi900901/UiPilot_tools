import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const variants = [
  { id: 'A', version: '1.0.0', mode: 'schema1' },
  { id: 'B', version: '1.0.0', mode: 'schema1' },
  { id: 'C-readiness-broken', version: '1.0.0', mode: 'missing-handler' },
  { id: 'T-controlled-throw', version: '1.0.0', mode: 'schema1' },
  { id: 'D-upgrade', version: '1.0.1', mode: 'schema1' },
  { id: 'V2-disposable', version: '1.0.0', mode: 'schema2' },
]

export async function createFixtures(outputRoot) {
  const source = new URL('../package/', import.meta.url)
  const manifest = JSON.parse((await readFile(new URL('plugin.json', source), 'utf8')).replace(/^\uFEFF/, ''))
  const runtime = await readFile(new URL('dist/runtime.js', source), 'utf8')
  // Fail rather than overwrite an earlier fixture set or acceptance evidence.
  await mkdir(outputRoot)
  const outputs = []
  for (const variant of variants) {
    const packageRoot = path.join(outputRoot, `${variant.id}-RECOVERY_SOURCE_PATH_SENTINEL`, 'package')
    await mkdir(path.join(packageRoot, 'dist'), { recursive: true })
    const candidate = structuredClone(manifest)
    candidate.version = variant.version
    const code = variant.mode === 'missing-handler'
      ? "// Valid static module, intentionally missing onCommand readiness export.\nexport const buildLabel = 'C-RECOVERY_ERROR_SENTINEL';\n"
      : runtime.replace("const BUILD_LABEL = 'A'", `const BUILD_LABEL = '${variant.id}'`)
        .replace("const DATA_MODE = 'schema1'", `const DATA_MODE = '${variant.mode}'`)
    await writeFile(path.join(packageRoot, 'plugin.json'), JSON.stringify(candidate, null, 2) + '\n')
    await writeFile(path.join(packageRoot, 'dist/runtime.js'), code)
    outputs.push({ ...variant, packageRoot })
  }
  return outputs
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw new Error('Usage: node scripts/create-fixtures.mjs <new-output-directory>')
  console.log(JSON.stringify(await createFixtures(path.resolve(process.argv[2])), null, 2))
}
