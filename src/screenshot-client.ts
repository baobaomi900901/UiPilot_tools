import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isScreenshotIdentity, type ScreenshotClient } from './screenshot-protocol'
export function createScreenshotClient(windowInstance: unknown): ScreenshotClient {
 if (!isScreenshotIdentity(windowInstance)) throw new Error('Invalid screenshot window identity')
 return {
  listenState: handler => listen('screenshot://state-changed', event => handler(event.payload)),
  getState: () => invoke('get_screenshot_state', {windowInstance}),
  readImage: (owner, transferId) => invoke<ArrayBuffer>('read_screenshot_image', {owner, transferId}),
  releaseImage: (owner, transferId) => invoke('release_screenshot_image', {owner, transferId}),
  copy: async owner => {
   const result = await invoke<{status:string}>('copy_screenshot',{owner})
   if(result?.status !== 'copied') throw {code:'unavailable'}
   return {status:'copied'}
  },
  save: async owner => {
   const result = await invoke<{status:string}>('save_screenshot',{owner})
   if(result?.status !== 'saved' && result?.status !== 'cancelled') throw {code:'unavailable'}
   return {status:result.status}
  },
  retake: owner => invoke('retake_screenshot',{owner}),
  close: owner => invoke('close_screenshot',{owner}),
 }
}
