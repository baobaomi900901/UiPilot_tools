import { useEffect, useRef, useState } from 'react'
import { Button, Checkbox, Form, Space } from 'antd'
import type { LoginItemStatus, MacLoginItemClient } from './macos-login-item'

const labels: Record<LoginItemStatus, string> = {
  notRegistered: '未启用', enabled: '已启用', requiresApproval: '等待系统允许',
  notFound: '系统尚未找到当前应用的登录项', unavailable: '无法读取系统登录项状态',
}
const issues = {
  invalidSignature: '当前应用签名不满足系统登录项要求，请使用符合签名要求的安装包。',
  permissionDenied: '系统未允许此登录项，请核对系统设置后刷新。',
  serviceUnavailable: '系统登录项服务暂不可用，请稍后刷新。',
  notFound: '系统仍未找到此应用的登录项，请确认从应用程序目录启动。',
  unknown: '系统未能完成此次变更，请核对状态后再操作。',
}
const uncertain = '变更未能确认，请核对系统登录项后刷新。'

export function MacLoginItemControl({ client, disabled = false }: { client?: MacLoginItemClient; disabled?: boolean }) {
  const [status, setStatus] = useState<LoginItemStatus>('unavailable')
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState('')
  const owner = useRef(0)
  const occupied = useRef(true)

  async function run(kind: 'read' | 'set' | 'open', enabled = false) {
    if (!client || occupied.current || (disabled && kind !== 'read')) return
    occupied.current = true
    const token = ++owner.current
    setBusy(true); setError('')
    let next: LoginItemStatus | undefined
    let notice = ''
    try {
      if (kind === 'read') next = (await client.read()).status
      else if (kind === 'open') await client.openSettings()
      else {
        const result = await client.set(enabled)
        next = result.status
        if (result.outcome === 'unconfirmed') notice = result.issue ? issues[result.issue] : uncertain
      }
    } catch {
      if (kind === 'set') {
        // A transport failure is not proof of rollback. Read facts; never retry.
        try { next = (await client.read()).status } catch { next = 'unavailable' }
        notice = uncertain
      } else if (kind === 'read') { next = 'unavailable' }
      else notice = '无法打开系统登录项设置，请稍后重试。'
    }
    if (owner.current !== token) return
    if (next !== undefined) setStatus(next)
    setError(notice); occupied.current = false; setBusy(false)
  }

  useEffect(() => {
    occupied.current = false
    if (client) void run('read')
    else { setStatus('unavailable'); setBusy(false) }
    return () => { ++owner.current; occupied.current = true }
  }, [client])

  const locked = disabled || busy || !client
  return <Form.Item label="登录时启动">
    <Space direction="vertical" size={8}>
      <Checkbox checked={status === 'enabled'} disabled={locked || !['enabled', 'notRegistered'].includes(status)}
        onChange={event => void run('set', event.target.checked)}>随登录启动 UiPilot</Checkbox>
      <span role="status">{busy ? '正在确认系统状态…' : labels[status]}</span>
      <Space size={8} wrap>
        <Button disabled={locked} onClick={() => void run('read')}>刷新</Button>
        {status === 'notFound' && <Button disabled={locked} onClick={() => void run('set', true)}>尝试启用</Button>}
        {status === 'requiresApproval' && <>
          <Button disabled={locked} onClick={() => void run('open')}>打开系统登录项</Button>
          <Button disabled={locked} onClick={() => void run('set', false)}>停止注册</Button>
        </>}
      </Space>
      {error && <span role="alert">{error}</span>}
    </Space>
  </Form.Item>
}
