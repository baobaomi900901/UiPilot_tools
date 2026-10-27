import { invoke } from '@tauri-apps/api/core'

type ProbeState =
  | 'invoke-pending'
  | 'invoke-succeeded'
  | 'acl-denied'
  | 'invoke-other-error'

declare global {
  interface Window {
    __UIPILOT_SECURITY_PROBE_STATE__?: ProbeState
  }
}

window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-pending'

try {
  void invoke('load_settings').then(
    () => {
      window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-succeeded'
    },
    (error: unknown) => {
      if (String(error) === 'Command load_settings not allowed by ACL') {
        window.__UIPILOT_SECURITY_PROBE_STATE__ = 'acl-denied'
        window.location.hash = 'acl-denied'
      } else {
        window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-other-error'
      }
    },
  )
} catch {
  window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-other-error'
}
