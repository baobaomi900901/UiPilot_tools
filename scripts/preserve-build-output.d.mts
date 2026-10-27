import type { Plugin } from 'vite'
import type { Buffer } from 'node:buffer'

type OutputSnapshot = Map<string, { content: Buffer; atime: Date; mtime: Date }>
export function snapshotOutput(root: string): OutputSnapshot
export function restoreUnchangedOutput(root: string, before: OutputSnapshot): number
export function preserveBuildOutput(): Plugin
