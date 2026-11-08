function receiver() { return this }
export function onCommand(invocation) {
  let undeclaredThrows = false
  let frozenWriteThrows = false
  try { UIPILOT_UNDECLARED_PROBE = true } catch { undeclaredThrows = true }
  try { invocation.input = 'changed' } catch { frozenWriteThrows = true }
  return {
    requestId: invocation.requestId,
    data: {
      unboundThis: receiver() === undefined,
      undeclaredThrows,
      noGlobal: typeof globalThis.UIPILOT_UNDECLARED_PROBE === 'undefined',
      frozenWriteThrows,
    },
  }
}
