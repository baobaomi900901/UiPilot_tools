import { readB } from './b.js'
export let a = 1
export function incrementA() { a += 1 }
export function snapshot() { return [a, readB()] }
