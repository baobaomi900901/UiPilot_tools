export function onCommand(invocation) {
  return { requestId: invocation.requestId, data: Object.fromEntries([
    'window', 'document', 'Audio', 'AudioContext', 'speechSynthesis', 'fetch',
    'XMLHttpRequest', 'WebSocket', 'Worker', 'TextEncoder', 'URL', 'setTimeout',
    'std', 'os', 'ObjC', 'require', 'process', '__finish', '__runner',
  ].map(name => [name, typeof globalThis[name]])) }
}
