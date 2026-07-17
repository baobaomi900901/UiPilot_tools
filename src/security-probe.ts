import { invoke } from '@tauri-apps/api/core'

invoke('load_settings').then(
  () => {
    window.location.hash = 'allowed'
  },
  () => {
    window.location.hash = 'rejected'
  },
)
