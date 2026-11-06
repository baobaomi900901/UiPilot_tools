// Verify the selected release artifact without launching it.
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { createReadStream, existsSync, lstatSync, mkdtempSync, openSync, closeSync, readSync,
  readFileSync, readdirSync, realpathSync, rmdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const run = (tool, ...args) => execFileSync(tool, args, { encoding: 'utf8' }).trim()
const json = file => JSON.parse(readFileSync(file, 'utf8'))
function version(value) {
  if (!/^\d+(\.\d+){0,2}$/.test(value)) throw new Error('Invalid system version: ' + value)
  const [major, minor = 0, patch = 0] = value.split('.').map(Number)
  return [major, minor, patch]
}
export function checkBinary(slices, commands, minimum, main) {
  if (slices.trim() !== 'arm64') throw new Error('Unexpected slices: ' + slices)
  const block = commands.split(/^Load command /m).find(part => /\bcmd LC_(BUILD_VERSION|VERSION_MIN_MACOSX)\b/.test(part))
  const modern = block?.includes('cmd LC_BUILD_VERSION')
  if (!block || (modern && !/^\s*platform\s+(1|macos)\s*$/im.test(block))) throw new Error('Missing macOS load command')
  const found = block.match(modern ? /^\s*minos\s+(\S+)/m : /^\s*version\s+(\S+)/m)?.[1]
  const declared = version(minimum)
  const difference = version(found).map((n,i) => n - declared[i]).find(n => n !== 0) ?? 0
  if (difference > 0 || (main && difference !== 0)) throw new Error('Incompatible deployment target: ' + found)
}
export function checkResourceName(relative) {
  if (/\.(exe|dll)$/i.test(relative) || /(^|\/)(everything|provider)(\/|$)/i.test(relative)) {
    throw new Error('Windows resource: ' + relative)
  }
}
const magic = new Set(['feedface','cefaedfe','feedfacf','cffaedfe','cafebabe','bebafeca','cafebabf','bfbafeca'])
function isMachO(file) {
  const fd = openSync(file, 'r')
  const header = Buffer.alloc(4)
  try { return readSync(fd, header, 0, 4, 0) === 4 && magic.has(header.toString('hex')) }
  finally { closeSync(fd) }
}
async function digest(file) {
  const hash = createHash('sha256')
  for await (const data of createReadStream(file)) hash.update(data)
  return hash.digest('hex')
}
export async function checkSharedResources(resources, bundleRoot, projectRoot) {
  for (const relative of resources) {
    const bundled = realpathSync(path.join(bundleRoot, relative))
    if (!bundled.startsWith(realpathSync(bundleRoot) + path.sep) ||
        await digest(bundled) !== await digest(path.join(projectRoot, relative))) {
      throw new Error('Incorrect bundled resource: ' + relative)
    }
  }
}
const bookmarkEntitlement = 'com.apple.security.files.bookmarks.app-scope'
export function checkDocumentsEntitlements(entitlements, main) {
  const keys = Object.keys(entitlements)
  if (main ? keys.length !== 1 || entitlements[bookmarkEntitlement] !== true : keys.length !== 0) {
    throw new Error('Unexpected Documents preview signing entitlements')
  }
}

function signedEntitlements(file) {
  const plist = execFileSync('codesign', ['--display', '--xml', '--entitlements', '-', file],
    {encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe']}).trim()
  if (!plist) return {}
  return JSON.parse(execFileSync('plutil', ['-convert', 'json', '-o', '-', '--', '-'],
    {input: plist, encoding: 'utf8'}))
}

async function inspectApp(app, documents) {
  app = realpathSync(app)
  const info = JSON.parse(run('plutil', '-convert','json','-o','-',path.join(app,'Contents/Info.plist')))
  const base = json(path.join(root,'src-tauri/tauri.conf.json'))
  const minimum = json(path.join(root,'src-tauri/tauri.macos.conf.json')).bundle.macOS.minimumSystemVersion
  if (info.CFBundleIdentifier !== base.identifier || info.CFBundleExecutable !== base.mainBinaryName ||
      info.CFBundleShortVersionString !== base.version || info.LSMinimumSystemVersion !== minimum) {
    throw new Error('Incorrect application identity, version or deployment target')
  }
  if (path.basename(info.CFBundleIconFile) !== info.CFBundleIconFile ||
      !existsSync(path.join(app,'Contents/Resources',info.CFBundleIconFile))) throw new Error('Missing application icon')
  const main = path.join(app,'Contents/MacOS',base.mainBinaryName)
  if (documents && !info.NSDocumentsFolderUsageDescription?.trim()) throw new Error('Missing Documents usage description')
  if (!lstatSync(main).isFile() || !isMachO(main)) throw new Error('Expected a regular Mach-O executable: ' + main)
  function visit(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name)
      checkResourceName(path.relative(app,file))
      if (entry.isSymbolicLink()) {
        if (!realpathSync(file).startsWith(app + path.sep)) throw new Error('External link: ' + file)
      } else if (entry.isDirectory()) visit(file)
      else if (entry.isFile() && isMachO(file)) {
        checkBinary(run('lipo','-archs',file), run('otool','-l',file), minimum, file === main)
        if (documents) checkDocumentsEntitlements(signedEntitlements(file), file === main)
      } else if (!entry.isFile()) throw new Error('Unexpected bundle entry: ' + file)
    }
  }
  visit(app)
  await checkSharedResources(base.bundle.resources, path.join(app, 'Contents/Resources'), path.join(root, 'src-tauri'))
  run('codesign','--verify','--deep','--strict',app)
  console.log('Verified signed arm64 app; minimum', minimum, 'executable SHA-256', await digest(main))
}
async function verify(input, documents) {
  if (process.platform !== 'darwin') throw new Error('Run on macOS')
  if (input.endsWith('.app')) return inspectApp(input, documents)
  if (!input.endsWith('.dmg') || !lstatSync(input).isFile()) throw new Error('Expected a regular .dmg file or an .app')
  const mount = mkdtempSync(path.join(realpathSync(tmpdir()),'uipilot-dmg-'))
  let attached = false
  try {
    run('hdiutil','attach','-readonly','-nobrowse','-mountpoint',mount,input)
    attached = true
    const apps = readdirSync(mount,{withFileTypes:true}).filter(entry => entry.isDirectory() && entry.name.endsWith('.app'))
    if (apps.length !== 1) throw new Error('Expected one top-level app in DMG')
    await inspectApp(path.join(mount,apps[0].name), documents)
  } finally {
    if (attached) run('hdiutil','detach',mount)
    if (existsSync(mount)) rmdirSync(mount)
  }
  console.log('DMG SHA-256', await digest(input))
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2)
    const documents = args[0] === '--documents'
    if (documents) args.shift()
    if (args.length > 1) throw new Error('Usage: node scripts/verify-macos-bundle.mjs [--documents] [artifact]')
    let input = args[0]
    if (!input) {
      const dir = path.join(process.env.CARGO_TARGET_DIR ?? path.join(root,'src-tauri/target'),'aarch64-apple-darwin/release/bundle/dmg')
      const files = readdirSync(dir).filter(name => name.endsWith('.dmg'))
      if (files.length !== 1) throw new Error('Provide the exact DMG path for this build')
      input = path.join(dir,files[0])
    }
    await verify(path.resolve(input), documents)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
