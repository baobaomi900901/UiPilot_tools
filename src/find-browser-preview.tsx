import { createRoot } from 'react-dom/client'

import { createFindCore } from './find-core'
import { FindView } from './find-view'
import type { FileResultItem, FindClient } from './protocol'

const RESULTS: readonly FileResultItem[] = [
  {
    resultId: 'preview-image',
    name: '香港.png',
    kind: 'file',
    sizeBytes: '2478',
    modifiedUtc: '2026-08-28T03:53:46Z',
    fullPath: String.raw`C:\Users\moby\Desktop\香港.png`,
  },
  {
    resultId: 'preview-folder-one',
    name: '香港',
    kind: 'folder',
    sizeBytes: null,
    modifiedUtc: '2026-08-27T10:01:10Z',
    fullPath: String.raw`C:\Users\moby\Desktop\香港`,
  },
  {
    resultId: 'preview-folder-two',
    name: '香港',
    kind: 'folder',
    sizeBytes: null,
    modifiedUtc: '2026-08-27T10:01:10Z',
    fullPath: String.raw`C:\Users\moby\Desktop\客户dbug数据\香港`,
  },
]

const thumbnailAsset = new URL('../src-tauri/icons/icon.png', import.meta.url)
let thumbnailDataUrl: Promise<string | null> | undefined
let previewRevision = 1n

function loadThumbnailDataUrl(): Promise<string | null> {
  thumbnailDataUrl ??= fetch(thumbnailAsset).then(async (response) => {
    if (!response.ok) return null
    const blob = await response.blob()
    return new Promise<string | null>((resolve) => {
      const reader = new FileReader()
      reader.addEventListener('load', () => resolve(typeof reader.result === 'string' ? reader.result : null))
      reader.addEventListener('error', () => resolve(null))
      reader.readAsDataURL(blob)
    })
  }, () => null)
  return thumbnailDataUrl
}

const client: FindClient = {
  listenForward: async (handler) => {
    queueMicrotask(() => handler({
      invocationId: 'browser-preview',
      forwardSequence: '1',
      query: '香港',
    }))
    return () => undefined
  },
  listenThemeChanged: async () => () => undefined,
  prepareInitialization: async () => ({
    status: 'prepared',
    initialization: {
      initializationToken: 'browser-preview-init',
      themeRevision: '1',
      theme: 'dark',
      filePreviewRevision: '1',
      filePreviewEnabled: true,
      pinned: false,
    },
  }),
  commitReady: async ({ initializationToken }) => ({ status: 'ready', initializationToken }),
  getReadyStatus: async ({ initializationToken }) => ({ status: 'ready', initializationToken }),
  searchFiles: async ({ category }) => {
    const items = category === 'all'
      ? RESULTS
      : category === 'folder'
        ? RESULTS.filter((item) => item.kind === 'folder')
        : category === 'image'
          ? RESULTS.filter((item) => item.kind === 'file')
          : []
    return {
      requestId: `browser-preview-${category}`,
      indexRevision: '1',
      total: String(items.length),
      status: 'ready',
      items: [...items],
    }
  },
  loadThumbnail: async ({ resultId }) => resultId === 'preview-image' ? loadThumbnailDataUrl() : null,
  executeResult: async () => ({ status: 'fileRevealRequested' }),
  setPinned: async ({ pinned }) => ({ pinned }),
  setPreviewPreference: async ({ preference }) => {
    previewRevision += 1n
    return {
      filePreviewRevision: String(previewRevision),
      filePreviewEnabled: preference.enabled,
    }
  },
  hide: async () => undefined,
}

const host = document.querySelector<HTMLElement>('#app')
if (!host) throw new Error('Missing preview root')

const core = createFindCore(client)
const root = createRoot(host)
root.render(<FindView core={core} />)

window.addEventListener('pagehide', () => {
  core.destroy()
  root.unmount()
}, { once: true })
