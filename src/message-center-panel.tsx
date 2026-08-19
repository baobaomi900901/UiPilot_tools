import { Button, Spin } from 'antd'
import { OverlayScrollbarsComponent } from 'overlayscrollbars-react'

import { PluginIcon } from './plugin-icon'
import { compareU64Decimal, type MessageCenterStateSnapshot } from './protocol'

const scrollbarOptions = {
  overflow: { x: 'hidden', y: 'scroll' },
  scrollbars: { theme: 'os-theme-uipilot', visibility: 'auto', autoHide: 'never' },
} as const

export function MessageCenterPanel({
  state,
  onClear,
}: {
  state: MessageCenterStateSnapshot
  onClear: () => void
}) {
  const messages = [...state.messages].sort((left, right) => compareU64Decimal(right.id, left.id))
  const unavailable = state.status === 'unavailable'
  const canClear = state.status === 'ready' && messages.length > 0 && !state.clearPending

  return (
    <OverlayScrollbarsComponent
      className="settings-tab-panel settings-message-panel"
      options={scrollbarOptions}
    >
      <div className="settings-scroll-content message-center-content-root">
        <header className="message-center-header">
          <h2>消息</h2>
          <Button
            disabled={!canClear}
            loading={state.clearPending}
            onClick={onClear}
            size="small"
          >
            清空全部
          </Button>
        </header>
        {unavailable ? (
          <div className="message-center-state" role="alert">消息不可用，请重启 UiPilot</div>
        ) : state.status === 'unknown' ? (
          <div className="message-center-state"><Spin size="small" /></div>
        ) : (
          <>
            {state.operationError ? (
              <div className="message-center-error" role="alert">MessageOperationFailed</div>
            ) : null}
            {messages.length === 0 ? (
              <div className="message-center-state">暂无消息</div>
            ) : (
              <div className="message-center-list" role="list" aria-label="消息记录">
                {messages.map((message) => (
                  <article
                    className={message.readAt === null ? 'message-center-row is-unread' : 'message-center-row'}
                    key={message.id}
                    role="listitem"
                  >
                    <PluginIcon iconUrl={message.pluginIconUrl} size={32} />
                    <div className="message-center-copy">
                      <div className="message-center-meta">
                        <span className="message-center-plugin">{message.pluginNameSnapshot}</span>
                        <time dateTime={message.createdAt}>{new Date(message.createdAt).toLocaleString()}</time>
                      </div>
                      <p className="message-center-content">{message.content}</p>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </OverlayScrollbarsComponent>
  )
}
