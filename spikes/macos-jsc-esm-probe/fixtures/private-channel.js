// Package code never gets the native completion callback or runner reference.
const finishType = typeof globalThis.__finish
const runnerType = typeof globalThis.__runner
let forged = false
try { globalThis.__finish('{"requestId":"request-1","data":"forged"}'); forged = true } catch {}
export async function onCommand(invocation) {
  return { requestId: invocation.requestId, data: { finishType, runnerType, forged } }
}
