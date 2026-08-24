import dgram from 'node:dgram'
import dns from 'node:dns'
import http from 'node:http'
import http2 from 'node:http2'
import https from 'node:https'
import net from 'node:net'
import { syncBuiltinESMExports } from 'node:module'
import tls from 'node:tls'

function blocked() {
  throw new Error('Network access is forbidden during the plugin CLI smoke test.')
}

for (const [target, names] of [
  [net, ['connect', 'createConnection']],
  [http, ['get', 'request']],
  [https, ['get', 'request']],
  [http2, ['connect']],
  [tls, ['connect']],
  [dgram, ['createSocket']],
  [dns, ['lookup', 'resolve', 'resolve4', 'resolve6', 'resolveAny']],
]) {
  for (const name of names) target[name] = blocked
}

for (const name of ['fetch', 'WebSocket', 'EventSource']) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value: blocked,
    writable: false,
  })
}

syncBuiltinESMExports()
