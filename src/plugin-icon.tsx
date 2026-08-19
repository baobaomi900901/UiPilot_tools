import { Plug } from 'lucide-react'
import { useEffect, useState } from 'react'

import { safePublicPluginIconUrl } from './plugin-icon-url'

export type PluginIconSize = 20 | 28 | 32 | 36

export function PluginIcon({
  iconUrl,
  size = 28,
  className = '',
}: {
  iconUrl?: string | null
  size?: PluginIconSize
  className?: string
}) {
  const safeUrl = safePublicPluginIconUrl(iconUrl)
  const [failed, setFailed] = useState(false)
  useEffect(() => setFailed(false), [safeUrl])
  const showImage = safeUrl !== undefined && !failed
  return (
    <span
      aria-hidden="true"
      className={`plugin-icon plugin-icon-${size}${className ? ` ${className}` : ''}`}
      data-plugin-icon-size={size}
    >
      <span className="plugin-icon-fallback" hidden={showImage}>
        <Plug size={Math.max(12, size - 8)} strokeWidth={1.8} />
      </span>
      {safeUrl ? (
        <img
          alt=""
          aria-hidden="true"
          className="plugin-icon-image"
          draggable={false}
          hidden={failed}
          onError={() => setFailed(true)}
          src={safeUrl}
        />
      ) : null}
    </span>
  )
}
