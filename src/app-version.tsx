import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'

/** Read metadata from the running host, independently of persisted settings. */
export function AppVersion() {
  const [version, setVersion] = useState('读取中…')

  useEffect(() => {
    let active = true
    void getVersion().then(
      (value) => { if (active) setVersion(value) },
      () => { if (active) setVersion('无法读取') },
    )
    return () => { active = false }
  }, [])

  return <span className="settings-version" aria-live="polite">版本 {version}</span>
}