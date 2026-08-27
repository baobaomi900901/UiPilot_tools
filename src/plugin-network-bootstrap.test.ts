import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapNetworkApi {
  request(input: unknown): Promise<unknown>
}

interface BootstrapRuntimeApi {
  readonly network?: Readonly<BootstrapNetworkApi>
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
    if (command === 'plugin_network_request') return requestResult(args)
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
