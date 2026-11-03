import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapNetworkApi {
  request(input: unknown): Promise<unknown>
}

interface BootstrapRuntimeApi {
  readonly network?: Readonly<BootstrapNetworkApi>
  readonly services?: { readonly youdao: { translate(input: unknown): Promise<unknown> } }
}

const context = Object.freeze({
  pluginId: 'com.example.network',
  pluginGeneration: 7,
  requestId: 'public-request-0000000000000001',
})

function runtimeBootstrapSource(
  networkEnabled: boolean,
  moduleBody = "handler = window.__TEST_HANDLER__; document.title = 'uipilot-public-plugin-ready';",
): string {
  const rust = readFileSync('src-tauri/src/public_plugins/runtime.rs', 'utf8')
  const template = rust.match(
    /PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u,
  )?.[1]
  if (!template) throw new Error('runtime bootstrap template is missing')
  return template
    .replace('__RESULT_ACTIONS__', 'false').replace('__WINDOW_ACTIONS__', 'false').replace('__PANEL_JSON__', readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8'))
    .replace('__PANEL_ACTIONS__', 'false')
    .replace('__RUNTIME_ACTIONS__', 'false')
    .replace('__NETWORK_HTTPS__', networkEnabled ? 'true' : 'false')
    .replace(
      /const entry = document\.documentElement\.dataset\.runtimeEntry;[\s\S]*?document\.title = 'uipilot-public-plugin-ready';/u,
      moduleBody,
    )
}

async function executeRuntimeBootstrap(
  networkEnabled: boolean,
  handler: (invocation: unknown, api: BootstrapRuntimeApi) => Promise<unknown>,
  requestResult: (input: unknown) => Promise<unknown> = async () => ({
    status: 200,
    headers: { 'x-result': ['ok'] },
    body: 'translated',
  }),
) {
  let eventCallback: ((message: { payload: unknown }) => Promise<void>) | null = null
  const invoke = vi.fn(async (command: string, args?: unknown): Promise<unknown> => {
    if (command === 'plugin:event|listen') return undefined
    if (command === 'plugin_network_request' || command === 'plugin_youdao_translate') return requestResult(args)
    return undefined
  })
  const hostWindow: Record<string, unknown> = {
    __TAURI_INTERNALS__: {
      invoke,
      transformCallback(callback: (message: { payload: unknown }) => Promise<void>) {
        eventCallback = callback
        return 1
      },
    },
    __TEST_HANDLER__: handler,
  }
  const document = {
    documentElement: { dataset: { runtimeEntry: '/runtime.js' } },
    title: '',
  }
  vm.runInNewContext(runtimeBootstrapSource(networkEnabled), {
    document,
    TextEncoder,
    Error,
    Object,
    Promise,
    Reflect,
    Set,
    TypeError,
    WeakSet,
    setTimeout,
    window: hostWindow,
  })
  await vi.waitFor(() => expect(document.title).toBe('uipilot-public-plugin-ready'))
  if (!eventCallback) throw new Error('runtime event callback was not registered')
  const dispatch = async () => {
    await eventCallback!({
      payload: {
        context,
        invocation: {
          apiVersion: 1,
          requestId: context.requestId,
          input: '/translate Hello',
          context: {
            platform: 'windows',
            theme: 'dark',
            invokedAt: '2026-08-28T00:00:00Z',
          },
        },
      },
    })
  }
  return { dispatch, invoke }
}

describe('public plugin Runtime network bootstrap', () => {
  it('dispatches the next handler during completion IPC without clearing its busy slot', async () => {
    let release: (() => void) | undefined
    const handler = vi.fn(async () => {
      if (handler.mock.calls.length === 2) {
        await new Promise<void>((resolve) => { release = resolve })
      }
      return { requestId: context.requestId, results: [] }
    })
    const runtime = await executeRuntimeBootstrap(true, handler)
    const original = runtime.invoke.getMockImplementation()!
    runtime.invoke.mockImplementation(async (command, args) => {
      if (command === 'complete_plugin_command' && handler.mock.calls.length === 1) {
        void runtime.dispatch()
      }
      return original(command, args)
    })
    await runtime.dispatch()
    expect(handler).toHaveBeenCalledTimes(2)
    await runtime.dispatch()
    expect(handler).toHaveBeenCalledTimes(2)
    release!()
  })

  it('reports completion once even when completion IPC rejects', async () => {
    const runtime = await executeRuntimeBootstrap(true, async () => ({ results: [] }))
    runtime.invoke.mockRejectedValueOnce(new Error('host unavailable'))
    await runtime.dispatch().catch(() => undefined)
    expect(
      runtime.invoke.mock.calls.filter(([command]) => command === 'complete_plugin_command'),
    ).toHaveLength(1)
  })

  it('exposes a frozen network API only for a declaring Runtime and snapshots input', async () => {
    let undeclaredApi: BootstrapRuntimeApi | undefined
    const undeclared = await executeRuntimeBootstrap(false, async (_invocation, api) => {
      undeclaredApi = api
      return { requestId: context.requestId, results: [] }
    })
    await undeclared.dispatch()
    expect(Object.prototype.hasOwnProperty.call(undeclaredApi!, 'network')).toBe(false)

    const input = {
      url: 'https://api.example.com/translate',
      method: 'POST',
      headers: { authorization: 'test-key' },
      body: { type: 'json', value: { text: 'Hello' } },
    }
    let declaredApi: BootstrapRuntimeApi | undefined
    let response: unknown
    const declared = await executeRuntimeBootstrap(true, async (_invocation, api) => {
      declaredApi = api
      const pending = api.network!.request(input)
      input.url = 'https://changed.example.com/'
      input.headers.authorization = 'changed'
      input.body.value.text = 'changed'
      response = await pending
      return { requestId: context.requestId, results: [] }
    })
    await declared.dispatch()

    expect(Object.isFrozen(declaredApi!.network)).toBe(true)
    expect(Object.isFrozen(response)).toBe(true)
    expect(declared.invoke).toHaveBeenCalledWith('plugin_network_request', {
      input: {
        context,
        request: {
          url: 'https://api.example.com/translate',
          method: 'POST',
          headers: { authorization: 'test-key' },
          body: { type: 'json', value: { text: 'Hello' } },
        },
      },
    })
  })

  it('rejects bad arity, unknown fields, and unsupported JSON before invoking Host network code', async () => {
    const failures: string[] = []
    const runtime = await executeRuntimeBootstrap(true, async (_invocation, api) => {
      const cyclic: Record<string, unknown> = {}
      cyclic.self = cyclic
      const calls = [
        () => (api.network!.request as (...args: unknown[]) => Promise<unknown>)(),
        () => (api.network!.request as (...args: unknown[]) => Promise<unknown>)(
          { url: 'https://api.example.com/', method: 'GET' },
          { url: 'https://api.example.com/', method: 'GET' },
        ),
        () => api.network!.request({ url: 'https://api.example.com/', method: 'GET', extra: true }),
        () => api.network!.request({
          url: 'https://api.example.com/',
          method: 'POST',
          body: { type: 'text', value: 'ok', extra: true },
        }),
        () => api.network!.request({
          url: 'https://api.example.com/',
          method: 'POST',
          body: { type: 'json', value: cyclic },
        }),
      ]
      for (const call of calls) {
        try {
          await call()
        } catch (error) {
          failures.push((error as Error).name)
        }
      }
      return { requestId: context.requestId, results: [] }
    })
    await runtime.dispatch()

    expect(failures).toEqual(Array(5).fill('InvalidNetworkRequestError'))
    expect(runtime.invoke).not.toHaveBeenCalledWith(
      'plugin_network_request',
      expect.anything(),
    )
  })

  it('keeps the bridge fail-closed after plugin module code replaces global built-ins', async () => {
    let eventCallback: ((message: { payload: unknown }) => Promise<void>) | null = null
    let report: { api: BootstrapRuntimeApi; response: unknown; invalidName: string; hostName: string } | null = null
    let networkCalls = 0
    const hostWindow: Record<string, unknown> = {}
    const invoke = vi.fn(async (command: string, args?: unknown): Promise<unknown> => {
      if (command === 'plugin:event|listen') return undefined
      if (command !== 'plugin_network_request') return undefined
      networkCalls += 1
      if (networkCalls === 2) throw hostWindow.__HOST_ERROR__
      return { status: 200, headers: { 'x-result': ['ok'] }, body: 'translated' }
    })
    hostWindow.__TAURI_INTERNALS__ = {
      invoke,
      transformCallback(callback: (message: { payload: unknown }) => Promise<void>) {
        eventCallback = callback
        return 1
      },
    }
    hostWindow.__REPORT__ = (
      api: BootstrapRuntimeApi,
      response: unknown,
      invalidName: string,
      hostName: string,
    ) => {
      report = { api, response, invalidName, hostName }
    }
    const document = {
      documentElement: { dataset: { runtimeEntry: '/runtime.js' } },
      title: '',
    }
    const moduleBody = `
      window.__HOST_ERROR__ = { code: 'expiredRequest' };
      Object.defineProperty(Error.prototype, 'name', { configurable: true, set() {} });
      Object.freeze = (value) => value;
      Object.assign = () => ({});
      Object.create = () => ({});
      Object.defineProperty = () => undefined;
      Object.getOwnPropertyDescriptors = () => ({});
      Object.getPrototypeOf = () => null;
      Object.hasOwn = () => false;
      Object.keys = () => [];
      Object.setPrototypeOf = () => undefined;
      Reflect.ownKeys = () => [];
      Array.isArray = () => true;
      Array.prototype.push = () => 0;
      Number.isFinite = () => false;
      RegExp.prototype.test = () => false;
      Set.prototype.has = () => false;
      WeakSet.prototype.add = () => undefined;
      WeakSet.prototype.delete = () => false;
      WeakSet.prototype.has = () => true;
      Error = function CompromisedError() {};
      handler = async (_invocation, api) => {
        const input = {
          url: 'https://api.example.com/translate',
          method: 'POST',
          body: { type: 'json', value: { values: ['Hello'] } },
        };
        const pending = api.network.request(input);
        input.url = 'https://changed.example.com/';
        input.body.value.values[0] = 'changed';
        const response = await pending;
        let invalidName = '';
        try {
          await api.network.request({ url: 'https://api.example.com/', method: 'GET', extra: true });
        } catch (error) {
          invalidName = error.name;
        }
        let hostName = '';
        try {
          await api.network.request({ url: 'https://api.example.com/', method: 'GET' });
        } catch (error) {
          hostName = error.name;
        }
        window.__REPORT__(api, response, invalidName, hostName);
        return { requestId: '${context.requestId}', results: [] };
      };
      document.title = 'uipilot-public-plugin-ready';
    `
    vm.runInNewContext(runtimeBootstrapSource(true, moduleBody), {
      document,
    TextEncoder,
      setTimeout,
      window: hostWindow,
    })
    await vi.waitFor(() => expect(document.title).toBe('uipilot-public-plugin-ready'))
    if (!eventCallback) throw new Error('runtime event callback was not registered')
    await (eventCallback as unknown as (message: { payload: unknown }) => Promise<void>)({
      payload: {
        context: {
          pluginId: 'com.example.network',
          pluginGeneration: 7,
          requestId: 'public-request-0000000000000001',
        },
        invocation: {
          apiVersion: 1,
          requestId: context.requestId,
          input: '/translate Hello',
          context: { platform: 'windows', theme: 'dark', invokedAt: '2026-08-28T00:00:00Z' },
        },
      },
    })

    expect(report).not.toBeNull()
    expect(Object.isFrozen(report!.api.network)).toBe(true)
    expect(Object.isFrozen(report!.response)).toBe(true)
    expect(report!.invalidName).toBe('InvalidNetworkRequestError')
    expect(report!.hostName).toBe('ExpiredRequestError')
    expect(invoke).toHaveBeenCalledWith('plugin_network_request', {
      input: {
        context: {
          pluginId: 'com.example.network',
          pluginGeneration: 7,
          requestId: 'public-request-0000000000000001',
        },
        request: {
          url: 'https://api.example.com/translate',
          method: 'POST',
          body: { type: 'json', value: { values: ['Hello'] } },
        },
      },
    })
  })

  it('maps only the nine exact Host codes and fails malformed errors closed', async () => {
    const codes = [
      'invalidNetworkRequest',
      'permissionDenied',
      'networkTargetDenied',
      'networkTimeout',
      'networkFailure',
      'networkResponseTooLarge',
      'networkResponseInvalid',
      'networkLimitExceeded',
      'expiredRequest',
    ] as const
    const expected = [
      'InvalidNetworkRequestError',
      'PermissionDeniedError',
      'NetworkTargetDeniedError',
      'NetworkTimeoutError',
      'NetworkFailureError',
      'NetworkResponseTooLargeError',
      'NetworkResponseInvalidError',
      'NetworkLimitExceededError',
      'ExpiredRequestError',
      'NetworkFailureError',
      'NetworkFailureError',
    ]
    const errors: unknown[] = [
      ...codes.map((code) => ({ code })),
      { code: 'networkTimeout', detail: 'private' },
      new Error('private dependency detail'),
    ]
    const names: string[] = []
    const errorCount = errors.length
    const runtime = await executeRuntimeBootstrap(
      true,
      async (_invocation, api) => {
        for (let index = 0; index < errorCount; index += 1) {
          try {
            await api.network!.request({ url: 'https://api.example.com/', method: 'GET' })
          } catch (error) {
            names.push((error as Error).name)
            expect((error as Error).message).not.toContain('private')
          }
        }
        return { requestId: context.requestId, results: [] }
      },
      async () => {
        throw errors.shift()
      },
    )
    await runtime.dispatch()
    expect(names).toEqual(expected)
  })

  it('rejects calls made after the command context expires without invoking Host network code', async () => {
    let savedApi: BootstrapRuntimeApi | undefined
    const runtime = await executeRuntimeBootstrap(true, async (_invocation, api) => {
      savedApi = api
      return { requestId: context.requestId, results: [] }
    })
    await runtime.dispatch()
    const before = runtime.invoke.mock.calls.filter(([command]) => command === 'plugin_network_request').length

    await expect(savedApi!.network!.request({
      url: 'https://api.example.com/',
      method: 'GET',
    })).rejects.toMatchObject({ name: 'ExpiredRequestError' })
    expect(runtime.invoke.mock.calls.filter(([command]) => command === 'plugin_network_request')).toHaveLength(before)
  })
})


describe('credential reference snapshot', () => {
  it('copies a credential ID and maps unavailable without provider detail', async () => {
    let failure = ''
    const input = { url: 'https://api.example.com/', method: 'GET', credential: 'service-token' }
    const runtime = await executeRuntimeBootstrap(true, async (_invocation, api) => {
      const pending = api.network!.request(input)
      input.credential = 'changed'
      try { await pending } catch (error) { failure = (error as Error).name }
      return { requestId: context.requestId, results: [] }
    }, async () => { throw { code: 'credentialUnavailable' } })
    await runtime.dispatch()
    expect(runtime.invoke).toHaveBeenCalledWith('plugin_network_request', {
      input: { context, request: { url: input.url, method: 'GET', credential: 'service-token' } },
    })
    expect(failure).toBe('CredentialUnavailableError')
  })

  it('rejects null, malformed IDs, getters, symbols and prototype keys without reading values', async () => {
    const getter = vi.fn(() => 'service-token')
    const base = { url: 'https://api.example.com/', method: 'GET' }
    const inputs = [
      ...[null, {}, '', 'Bad', 'has space', 'a'.repeat(65)].map(credential => ({ ...base, credential })),
      Object.defineProperty({ ...base }, 'credential', { get: getter, enumerable: true }),
      { ...base, credential: 'service-token', [Symbol('extra')]: true },
      Object.defineProperty({ ...base, credential: 'service-token' }, '__proto__', { value: 'bad' }),
    ]
    const failures: string[] = []
    const runtime = await executeRuntimeBootstrap(true, async (_invocation, api) => {
      for (const input of inputs) {
        try { await api.network!.request(input) } catch (error) { failures.push((error as Error).name) }
      }
      return { requestId: context.requestId, results: [] }
    })
    await runtime.dispatch()
    expect(failures).toEqual(inputs.map(() => 'InvalidNetworkRequestError'))
    expect(getter).not.toHaveBeenCalled()
    expect(runtime.invoke.mock.calls.filter(([command]) => command === 'plugin_network_request')).toEqual([])
  })
})


describe('host Youdao service facade', () => {
 it('snapshots input, freezes output and expires retained APIs', async () => {
  let retained: BootstrapRuntimeApi | undefined
  let output: unknown
  const input = { text: 'hello', from: 'en', to: 'zh-CHS' }
  const runtime = await executeRuntimeBootstrap(true, async (_, api) => {
   retained = api
   const pending = api.services!.youdao.translate(input)
   input.text = 'changed'
   output = await pending
   return { results: [] }
  }, async () => ({ translations: ['你好'] }))
  await runtime.dispatch()
  expect(Object.isFrozen(retained!.services!.youdao)).toBe(true)
  expect(Object.isFrozen(output)).toBe(true)
  expect(Object.isFrozen((output as {translations: string[]}).translations)).toBe(true)
  expect(runtime.invoke).toHaveBeenCalledWith('plugin_youdao_translate', {input: {context, input: {text: 'hello', from: 'en', to: 'zh-CHS'}}})
  await expect(retained!.services!.youdao.translate(input)).rejects.toMatchObject({name: 'ExpiredRequestError'})
 })
 it('rejects getters and invalid UTF-8 length, language and fields before IPC', async () => {
  let getterCalls = 0
  const runtime = await executeRuntimeBootstrap(true, async (_, api) => {
   for (const input of [
    { get text() { getterCalls++; return 'hello' }, from: 'en', to: 'zh-CHS' },
    { text: '', from: 'en', to: 'zh-CHS' },
    { text: '中'.repeat(2731), from: 'en', to: 'zh-CHS' },
    { text: 'hello', from: 'en', to: 'en' },
    { text: 'hello', from: 'en', to: 'zh-CHS', url: 'https://example.com' },
   ]) await expect(api.services!.youdao.translate(input)).rejects.toMatchObject({name: 'InvalidServiceRequestError'})
   return { results: [] }
  })
  await runtime.dispatch()
  expect(getterCalls).toBe(0)
  expect(runtime.invoke.mock.calls.some(([command]) => command === 'plugin_youdao_translate')).toBe(false)
 })
 it.each([
  ['servicePermissionDenied', 'ServicePermissionDeniedError'], ['credentialNotConfigured', 'CredentialNotConfiguredError'],
  ['credentialDisabled', 'CredentialDisabledError'], ['credentialUnavailable', 'CredentialUnavailableError'],
  ['providerAuthentication', 'ProviderAuthenticationError'], ['providerQuota', 'ProviderQuotaError'], ['providerFailure', 'ProviderFailureError'],
 ])('maps %s to fixed %s', async (code, name) => {
  const runtime = await executeRuntimeBootstrap(true, async (_, api) => {
   await expect(api.services!.youdao.translate({text:'hello',from:'en',to:'zh-CHS'})).rejects.toMatchObject({name, message: name})
   return {results:[]}
  }, async () => {throw {code}})
  await runtime.dispatch()
 })
})
