import assert from 'node:assert/strict'
import test from 'node:test'
import { checkBinary, checkResourceName } from './verify-macos-bundle.mjs'

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
