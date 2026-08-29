import type { PluginHandler } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const translateHandler: PluginHandler = async (invocation, api) => {
  if (!api.network) {
    return {
      requestId: invocation.requestId,
      results: [{ id: 'translate-error', title: 'Network unavailable' }],
    }
  }

  const response = await api.network.request({
    url: 'https://openapi.youdao.com/api',
    method: 'POST',
    headers: { accept: 'application/json' },
    body: {
      type: 'form',
      value: {
        q: invocation.input,
        from: 'en',
        to: 'zh-CHS',
        appKey: 'test-only',
        salt: 'test-only',
        sign: 'test-only',
        signType: 'v3',
        curtime: '0',
      },
    },
  })

  return {
    requestId: invocation.requestId,
    results: [
      {
        id: 'translate-result',
        title: response.body,
        defaultAction: { type: 'copyText', text: response.body },
      },
    ],
  }
}

void translateHandler
