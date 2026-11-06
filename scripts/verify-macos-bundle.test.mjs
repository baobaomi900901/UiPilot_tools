import assert from 'node:assert/strict'
import test from 'node:test'
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { checkBinary, checkResourceName, checkSharedResources } from './verify-macos-bundle.mjs'

const macho = minos => 'Load command 9\n  cmd LC_BUILD_VERSION\n  platform 1\n  minos ' + minos + '\n'
test('only the main executable must match the exact minimum', () => {
  checkBinary('arm64', macho('15.7.4'), '15.7.4', true)
  checkBinary('arm64', macho('13.0'), '15.7.4', false)
  assert.throws(() => checkBinary('arm64', macho('13.0'), '15.7.4', true))
})
test('rejects universal binaries, incompatible nested binaries, and unidentified targets', () => {
  for (const [arch, load] of [['x86_64 arm64', macho('15.7.4')], ['arm64', macho('15.10')], ['arm64', macho('15.7.4').replace('platform 1','platform 2')], ['arm64', '']]) {
    assert.throws(() => checkBinary(arch, load, '15.7.4', false))
  }
})
test('rejects Windows tools even when copied inside a nested resources directory', () => {
  for (const name of ['nested/Provider.EXE','Resources/x.DLL','Resources/everything/LICENSE.txt']) {
    assert.throws(() => checkResourceName(name))
  }
  checkResourceName('Contents/Resources/resources/sounds/message-notification.wav')
})

test('shared resources must be shipped inside the bundle with the original bytes', async t => {
  const temp = mkdtempSync(path.join(tmpdir(), 'uipilot-shared-resource-'))
  t.after(() => rmSync(temp, { recursive: true, force: true }))
  const bundle = path.join(temp, 'bundle')
  mkdirSync(bundle)
  const name = 'expected.wav'
  const input = path.join(temp, name)
  const output = path.join(bundle, name)
  writeFileSync(input, 'original bytes')
  const check = () => checkSharedResources([name], bundle, temp)
  await assert.rejects(check, /ENOENT/)
  writeFileSync(output, 'different bytes')
  await assert.rejects(check, /Incorrect bundled resource/)
  unlinkSync(output)
  symlinkSync(input, output)
  await assert.rejects(check, /Incorrect bundled resource/)
  unlinkSync(output)
  writeFileSync(output, 'original bytes')
  await check()
})
