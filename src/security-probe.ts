import { invoke } from '@tauri-apps/api/core'

invoke('load_settings').catch((error: unknown) => {
  if (String(error) === 'Command load_settings not allowed by ACL') {
    window.location.hash = 'acl-denied'
  }
})
