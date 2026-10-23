import type {
  PluginNetworkErrorName,
  PluginNetworkCredential,
  PluginNetworkRequest,
  PluginNetworkResponse,
  UiPilotPluginApiV1,
} from '../uipilot-plugin-api-v1'

declare const api: Readonly<UiPilotPluginApiV1>

const requests: readonly PluginNetworkRequest[] = [
  { url: 'https://api.example.com/value', method: 'GET' },
  { url: 'https://api.example.com/value', method: 'POST' },
  {
    url: 'https://api.example.com/value',
    method: 'POST',
    body: { type: 'json', value: { text: 'Hello' } },
  },
  {
    url: 'https://api.example.com/value',
    method: 'POST',
    body: { type: 'text', value: 'Hello' },
  },
  {
    url: 'https://api.example.com/value',
    method: 'POST',
    headers: { authorization: 'test-only' },
    body: { type: 'form', value: { q: 'Hello' } },
  },
]

async function requestAll(): Promise<readonly PluginNetworkResponse[]> {
  if (!api.network) return []
  return Promise.all(requests.map((request) => api.network!.request(request)))
}

function narrowNetworkError(error: unknown): PluginNetworkErrorName | null {
  if (!(error instanceof Error)) return null
  const names = new Set<PluginNetworkErrorName>([
    'InvalidNetworkRequestError',
    'PermissionDeniedError',
    'NetworkTargetDeniedError',
    'NetworkTimeoutError',
    'NetworkFailureError',
    'NetworkResponseTooLargeError',
    'NetworkResponseInvalidError',
    'NetworkLimitExceededError',
    'ExpiredRequestError',
  ])
  return names.has(error.name as PluginNetworkErrorName)
    ? (error.name as PluginNetworkErrorName)
    : null
}

void requestAll
void narrowNetworkError

const credential: PluginNetworkCredential = { id: 'token', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' }
const credentialRequest: PluginNetworkRequest = { url: 'https://api.example.com', method: 'GET', credential: credential.id }
// @ts-expect-error Only bearer metadata is supported.
const invalidCredential: PluginNetworkCredential = { ...credential, type: 'basic' }
// @ts-expect-error Runtime submits an ID, never credential metadata.
const invalidCredentialRequest: PluginNetworkRequest = { ...credentialRequest, credential }
// @ts-expect-error Null is not an absent credential.
const nullCredentialRequest: PluginNetworkRequest = { ...credentialRequest, credential: null }
void invalidCredential
void invalidCredentialRequest
void nullCredentialRequest
