import { a, incrementA } from './a.js'
export let b = 10
export function readB() { return b }
export function advance() { incrementA(); b += a }
