// Generator replaces only these two fixed build constants.
const BUILD_LABEL = 'A'
const DATA_MODE = 'schema1'
const STORAGE_KEY = 'recovery-snapshot'
const MAX_COUNT = 1_000_000
const PRIVATE_SENTINEL = 'RECOVERY_PRIVATE_SENTINEL_V2'

function validSnapshot(value, schemaVersion) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const keys = Object.keys(value).sort().join(',')
  const expected = schemaVersion === 1 ? 'count,schemaVersion' : 'count,schemaVersion,sentinel'
  return keys === expected && value.schemaVersion === schemaVersion &&
    Number.isSafeInteger(value.count) && value.count >= 0 && value.count <= MAX_COUNT &&
    (schemaVersion === 1 || value.sentinel === PRIVATE_SENTINEL)
}

function response(requestId, code, message, setting = null) {
  // Ordinary settings are displayed as bounded plain text, never used as code.
  const label = typeof setting === 'string' ? Array.from(setting).slice(0, 128).join('') : '不可用'
  return { requestId, results: [{
    id: 'recovery-status',
    title: `构建 ${BUILD_LABEL} · ${code}`,
    subtitle: message,
    detail: `RECOVERY_CONTENT_SENTINEL\n构建：${BUILD_LABEL}\n${message}\n默认设置：${label}`,
  }] }
}

export async function onCommand(invocation, api) {
  if (invocation.commandId !== 'recovery') throw new Error('Unsupported command ID')
  const input = invocation.input.trim() || 'status'
  if (input === 'throw') throw new Error('RECOVERY_ERROR_SENTINEL controlled runtime failure')
  const v2 = DATA_MODE === 'schema2'
  if (!['status', 'inc', ...(v2 ? ['seed-v2', 'inspect-v2'] : [])].includes(input)) {
    return response(invocation.requestId, 'help', v2 ? '输入 status / inc / throw / seed-v2 / inspect-v2' : '输入 status / inc / throw')
  }
  let previous, setting
  try {
    previous = await api.storage.get(STORAGE_KEY)
    setting = await api.settings.get('label')
  } catch {
    return response(invocation.requestId, 'read-unavailable', '读取失败；未尝试写入，请重新运行 status 核对。')
  }
  const absent = previous === null
  const schema1 = validSnapshot(previous, 1)
  const schema2 = validSnapshot(previous, 2)
  const readable = absent || schema1 || (v2 && schema2)
  if (!readable) {
    return response(invocation.requestId, 'write-blocked', '数据版本不兼容或格式无效；保留原数据，禁止写入。', setting)
  }
  const count = absent ? 0 : previous.count
  if (input === 'status' || input === 'inspect-v2') {
    const dataLabel = absent ? '未存储（仅内存计数 0）' : `schema ${previous.schemaVersion}；计数 ${count}`
    const sentinel = input === 'inspect-v2' && schema2 ? `；sentinel=${previous.sentinel}` : ''
    return response(invocation.requestId, 'status', `${dataLabel}${sentinel}；只读`, setting)
  }
  if (input === 'inc' && schema2) {
    return response(invocation.requestId, 'write-blocked', 'schema 2 仅供显式种子与只读核对；不递增。', setting)
  }
  if (input === 'inc' && count === MAX_COUNT) {
    return response(invocation.requestId, 'limit', '计数达到上限；保留原数据。', setting)
  }
  // No migration/reset/remove or multi-key transaction. seed-v2 is explicitly disposable.
  const next = input === 'seed-v2'
    ? { schemaVersion: 2, count, sentinel: PRIVATE_SENTINEL }
    : { schemaVersion: 1, count: count + 1 }
  try {
    await api.storage.set(STORAGE_KEY, next)
  } catch {
    return response(invocation.requestId, 'save-uncertain', '保存结果待确认；可能已经写入。先运行 status，核对后再主动重试。', setting)
  }
  return response(invocation.requestId, 'saved', `schema ${next.schemaVersion}；计数 ${next.count}；已保存一次`, setting)
}
