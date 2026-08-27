import { describe, expect, it } from 'vitest'

import {
  parsePluginTimerStartInput,
  parsePluginTimerState,
  parsePublicPluginInventory,
  parsePublicPluginPrepareSummary,
} from './protocol'

describe('public plugin timer protocol', () => {
  it('accepts only exact bounded timer start input', () => {
    expect(parsePluginTimerStartInput({ durationMs: 1_000, completionMessage: ' done ' })).toEqual({
      durationMs: 1_000,
      completionMessage: ' done ',
    })
    expect(parsePluginTimerStartInput({ durationMs: 86_400_000, completionMessage: 'x'.repeat(500) })).not.toBeNull()

    for (const invalid of [
      { durationMs: 999, completionMessage: 'done' },
      { durationMs: 86_400_001, completionMessage: 'done' },
      { durationMs: 1_000.5, completionMessage: 'done' },
      { durationMs: Number.MAX_SAFE_INTEGER + 1, completionMessage: 'done' },
      { durationMs: 1_000, completionMessage: '   ' },
      { durationMs: 1_000, completionMessage: 'line\nbreak' },
      { durationMs: 1_000, completionMessage: 'x'.repeat(501) },
      { durationMs: 1_000, completionMessage: 'done', extra: true },
    ]) {
      expect(parsePluginTimerStartInput(invalid)).toBeNull()
    }
  })

  it('parses exact timer states with canonical u64 revisions', () => {
    expect(parsePluginTimerState({
      timerRevision: '0', phase: 'idle', durationMs: null, remainingMs: null,
    })).toEqual({ timerRevision: '0', phase: 'idle', durationMs: null, remainingMs: null })
    expect(parsePluginTimerState({
      timerRevision: '10', phase: 'running', durationMs: 10_000, remainingMs: 9_000,
    })).not.toBeNull()
    expect(parsePluginTimerState({
      timerRevision: '18446744073709551615', phase: 'fired', durationMs: 10_000, remainingMs: 0,
    })).not.toBeNull()

    for (const invalid of [
      { timerRevision: '01', phase: 'idle', durationMs: null, remainingMs: null },
      { timerRevision: '1', phase: 'claiming', durationMs: 10_000, remainingMs: 0 },
      { timerRevision: '1', phase: 'running', durationMs: 10_000, remainingMs: null },
      { timerRevision: '1', phase: 'running', durationMs: 10_000, remainingMs: 10_001 },
      { timerRevision: '1', phase: 'fired', durationMs: 10_000, remainingMs: 1 },
      { timerRevision: '1', phase: 'idle', durationMs: null, remainingMs: null, extra: true },
    ]) {
      expect(parsePluginTimerState(invalid)).toBeNull()
    }
  })

  it('accepts timer.control in public plugin inventory permissions', () => {
    const inventory = {
      revision: '1',
      items: [{
        pluginId: 'com.example.timer',
        name: 'Timer',
        description: null,
        version: '1.0.0',
        source: 'localPackage',
        defaultName: 'timer',
        effectiveName: 'timer',
        enabled: true,
        fault: null,
        generation: 1,
        iconUrl: null,
        network: null,
        permissions: [{ permission: 'timer.control', supported: true, granted: true }],
        settings: [],
      }],
    }
    expect(parsePublicPluginInventory(inventory)).toEqual(inventory)
  })

  it('accepts ui.panel in public plugin inventory permissions', () => {
    const inventory = {
      revision: '1',
      items: [{
        pluginId: 'com.example.panel',
        name: 'Panel',
        description: null,
        version: '1.0.0',
        source: 'localPackage',
        defaultName: 'panel',
        effectiveName: 'panel',
        enabled: true,
        fault: null,
        generation: 1,
        iconUrl: null,
        network: null,
        permissions: [{ permission: 'ui.panel', supported: true, granted: true }],
        settings: [],
      }],
    }
    expect(parsePublicPluginInventory(inventory)).toEqual(inventory)
  })

  it('strictly parses sorted exact HTTPS hosts in inventory and prepare summaries', () => {
    const item = {
      pluginId: 'com.example.network',
      name: 'Network',
      description: null,
      version: '1.0.0',
      source: 'localPackage',
      defaultName: 'network',
      effectiveName: 'network',
      enabled: true,
      fault: null,
      generation: 1,
      iconUrl: null,
      network: { httpsHosts: ['api.example.com', 'auth.example.com'] },
      permissions: [{ permission: 'network.https', supported: true, granted: true }],
      settings: [],
    }
    expect(parsePublicPluginInventory({ revision: '1', items: [item] })).toEqual({
      revision: '1',
      items: [item],
    })
    for (const network of [
      { httpsHosts: ['auth.example.com', 'api.example.com'] },
      { httpsHosts: ['api.example.com', 'api.example.com'] },
      { httpsHosts: ['https://api.example.com'] },
      { httpsHosts: ['127.0.0.1'] },
      { httpsHosts: ['xn--bcher-kva.example'] },
      { httpsHosts: ['api.example.com'], extra: true },
      { httpsHosts: [] },
    ]) {
      expect(parsePublicPluginInventory({ revision: '1', items: [{ ...item, network }] })).toBeNull()
    }

    const prepared = {
      token: 'public-prepare-0000000000000001-0000000000000002',
      pluginId: 'com.example.network',
      name: 'Network',
      version: '1.1.0',
      permissions: ['network.https'],
      isUpdate: true,
      sourceVerified: false,
      iconUrl: null,
      network: {
        httpsHosts: ['api.example.com', 'auth.example.com'],
        addedHttpsHosts: ['auth.example.com'],
        requiresNetworkConsent: true,
      },
    }
    expect(parsePublicPluginPrepareSummary(prepared)).toEqual(prepared)
    expect(parsePublicPluginPrepareSummary({ ...prepared, permissions: [], network: null })).not.toBeNull()
    expect(parsePublicPluginPrepareSummary({
      ...prepared,
      isUpdate: false,
      network: { ...prepared.network, addedHttpsHosts: ['auth.example.com'] },
    })).toBeNull()
    for (const network of [
      { ...prepared.network, addedHttpsHosts: ['other.example.com'] },
      { ...prepared.network, addedHttpsHosts: [], requiresNetworkConsent: true },
      { ...prepared.network, requiresNetworkConsent: false },
      { ...prepared.network, httpsHosts: ['auth.example.com', 'api.example.com'] },
      { ...prepared.network, extra: true },
    ]) {
      expect(parsePublicPluginPrepareSummary({ ...prepared, network })).toBeNull()
    }
  })
})
