import { useEffect, useState } from 'react'
import type { CredentialInventory } from './credential-protocol'
import type { LauncherClient, PublicPermission } from './protocol'

export const HOST_CREDENTIALS = {
  'credentials.youdaoTranslation': { service: 'youdaoTranslation', label: '有道翻译密钥', english: 'Youdao Translation' },
  'credentials.baiduOcr': { service: 'baiduOcr', label: '百度 OCR 密钥', english: 'Baidu OCR' },
  'credentials.deepseek': { service: 'deepseek', label: 'DeepSeek 密钥', english: 'DeepSeek' },
} as const
export type HostCredentialPermission = keyof typeof HOST_CREDENTIALS
export function isHostCredentialPermission(permission: PublicPermission): permission is HostCredentialPermission {
  return Object.prototype.hasOwnProperty.call(HOST_CREDENTIALS, permission)
}
export function installationPermissionGrants(permissions: readonly PublicPermission[]) {
  return permissions.filter((permission) => !isHostCredentialPermission(permission) && permission !== 'selection.read')
}
export const HOST_CREDENTIAL_CONSENT = '允许此插件通过主程序使用对应官方服务，可能消耗 API 额度。此授权不提供密钥明文，也不会开启全局配置。'

// Keep global configuration availability separate from each plugin's grant.
export function useHostCredentialStatus(client: LauncherClient, active: boolean, revision: unknown) {
  const [state, setState] = useState<{ inventory: CredentialInventory | null; failed: boolean }>({ inventory: null, failed: false })
  useEffect(() => {
    if (!active) return
    let live = true
    setState({ inventory: null, failed: false })
    if (!client.credentials) { setState({ inventory: null, failed: true }); return }
    void client.credentials.list().then((inventory) => { if (live) setState({ inventory, failed: false }) }).catch(() => { if (live) setState({ inventory: null, failed: true }) })
    return () => { live = false }
  }, [client, active, revision])
  return (permission: HostCredentialPermission) => {
    if (state.failed) return '配置状态加载失败'
    if (!state.inventory) return '正在读取配置…'
    const entry = state.inventory.items.find((item) => item.service === HOST_CREDENTIALS[permission].service)
    return !entry?.configured ? '尚未配置' : !entry.enabled ? '配置已关闭' : '配置就绪'
  }
}

export const SELECTION_CONSENT = '仅在你主动按下划词翻译快捷键时取词。无法直接读取时，主程序会临时复制、有条件恢复原剪贴板，并处理 UiPilot 与 Windows 的临时历史记录；期间新复制的内容会保留。原剪贴板仅在主程序内存中缓存，不交给插件。不会后台监控选择，也不授予插件任意读取剪贴板的能力。'
