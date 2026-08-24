import { Button, Tooltip } from 'antd'
import { Pin, X } from 'lucide-react'
import { useSyncExternalStore } from 'react'
import { PluginIcon } from './plugin-icon'
import type { PluginWindowCore } from './plugin-window-core'

export function PluginWindowView({ core }: { core: PluginWindowCore }) {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  return (
    <header className="plugin-window-shell" data-tauri-drag-region>
      <PluginIcon className="plugin-window-icon" iconUrl={snapshot.iconUrl} size={20} />
      <strong className="plugin-window-title" data-tauri-drag-region>{snapshot.name}</strong>
      <span className="plugin-window-shell-spacer" data-tauri-drag-region />
      <Tooltip title={snapshot.pinned ? '取消固定' : '固定窗口'}>
        <Button
          aria-label={snapshot.pinned ? '取消固定' : '固定窗口'}
          aria-pressed={snapshot.pinned}
          className={snapshot.pinned ? 'is-selected' : undefined}
          disabled={snapshot.pending}
          icon={(
            <Pin
              aria-hidden
              fill={snapshot.pinned ? 'currentColor' : 'none'}
              size={16}
              strokeWidth={1.8}
            />
          )}
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
          icon={<X aria-hidden size={16} strokeWidth={1.8} />}
          onClick={() => void core.close()}
          size="small"
          type="text"
        />
      </Tooltip>
      {snapshot.error ? <span className="visually-hidden" role="status">{snapshot.error}</span> : null}
    </header>
  )
}
