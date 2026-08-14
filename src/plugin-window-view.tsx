import { CloseOutlined, PushpinFilled, PushpinOutlined } from '@ant-design/icons'
import { Button, Tooltip } from 'antd'
import { useSyncExternalStore } from 'react'
import type { PluginWindowCore } from './plugin-window-core'

export function PluginWindowView({ core }: { core: PluginWindowCore }) {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  return (
    <header className="plugin-window-shell" data-tauri-drag-region>
      <strong data-tauri-drag-region>UiPilot</strong>
      <span className="plugin-window-shell-spacer" data-tauri-drag-region />
      <Tooltip title={snapshot.pinned ? '取消固定' : '固定窗口'}>
        <Button
          aria-label={snapshot.pinned ? '取消固定' : '固定窗口'}
          className={snapshot.pinned ? 'is-selected' : undefined}
          disabled={snapshot.pending}
          icon={snapshot.pinned ? <PushpinFilled /> : <PushpinOutlined />}
          onClick={() => void core.togglePinned()}
          size="small"
          type="text"
        />
      </Tooltip>
      <Tooltip title="关闭">
        <Button
          aria-label="关闭"
          danger
          disabled={snapshot.pending}
          icon={<CloseOutlined />}
          onClick={() => void core.close()}
          size="small"
          type="text"
        />
      </Tooltip>
      {snapshot.error ? <span className="visually-hidden" role="status">{snapshot.error}</span> : null}
    </header>
  )
}