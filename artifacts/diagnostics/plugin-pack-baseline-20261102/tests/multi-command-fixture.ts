import { readFileSync } from 'node:fs'
import type { PublicManifestV1, PublicNamedCommandV1 } from '../src/manifest.js'

export function multiCommandManifest(): PublicManifestV1 & { commands: PublicNamedCommandV1[] } {
  return JSON.parse(readFileSync(new URL('./fixtures/multi-command/plugin.json', import.meta.url), 'utf8'))
}
