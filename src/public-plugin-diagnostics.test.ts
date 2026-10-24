import { describe, expect, it } from 'vitest'
import { formatPublicPluginDiagnostics, parsePublicPluginDiagnostics } from './protocol'

export const diagnosticFixture = {
  schemaVersion: 1, hostVersion: '0.7.0', platform: 'windows', pluginId: 'com.example.recovery',
  state: { status: 'installed', packageVersion: '1.0.0', generation: '1', inventoryRevision: '2', enabled: true, fault: null },
  lastOperation: { operation: 'developmentRefresh', phase: 'complete', errorCode: null, stateCommitOutcome: 'committed' },
} as const

describe('diagnostic allowlist', () => {
  it('rebuilds exact preview order and excludes all extra fields at every level', () => {
    const report = parsePublicPluginDiagnostics(diagnosticFixture)!
    expect(report).not.toBeNull()
    expect(formatPublicPluginDiagnostics(report)).toBe(JSON.stringify(diagnosticFixture, null, 2))
    for (const key of ['settings', 'path', 'secret', 'content', 'requestId', 'error']) {
      expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, [key]: 'PRIVATE_SENTINEL' })).toBeNull()
      expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, state: { ...diagnosticFixture.state, [key]: 'PRIVATE_SENTINEL' } })).toBeNull()
      expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, lastOperation: { ...diagnosticFixture.lastOperation, [key]: 'PRIVATE_SENTINEL' } })).toBeNull()
    }
  })
  it('rejects noncanonical scalars, bounds, enums and contradictory status', () => {
    for (const generation of ['01', '-1', '+1', '0', '18446744073709551616', 1]) expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, state: { ...diagnosticFixture.state, generation } })).toBeNull()
    for (const hostVersion of ['+0.7.0', '00.7.0', '0.7.0-beta', '4294967296.0.0']) expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, hostVersion })).toBeNull()
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, pluginId: 'a'.repeat(65) })).toBeNull()
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, state: { ...diagnosticFixture.state, fault: 'runtimeUnavailable' } })).toBeNull()
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, lastOperation: { ...diagnosticFixture.lastOperation, errorCode: 'PRIVATE_SENTINEL' } })).toBeNull()
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, lastOperation: { ...diagnosticFixture.lastOperation, phase: 'publication', stateCommitOutcome: 'notCommitted' } })).toBeNull()
  })
  it('rejects accessors without reading them and separates historical observation from current state', () => {
    let reads = 0
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, get pluginId() { reads++; return 'com.example.recovery' } })).toBeNull()
    expect(reads).toBe(0)
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, lastOperation: { operation: 'replace', phase: 'persistence', errorCode: 'pluginUnavailable', stateCommitOutcome: 'unknown' } })).not.toBeNull()
    expect(parsePublicPluginDiagnostics({ ...diagnosticFixture, lastOperation: { operation: 'prepare', phase: 'complete', errorCode: null, stateCommitOutcome: 'notAttempted' } })).not.toBeNull()
  })
})
