export function onCommand(invocation) {
  globalThis.retainedAllocation = new Uint8Array(4 * 1024 * 1024)
  // Touch each page; this is a real allocation, not a sparse JS array length.
  for (let i = 0; i < retainedAllocation.length; i += 4096) retainedAllocation[i] = 7
  return { requestId: invocation.requestId, data: retainedAllocation.byteLength }
}
