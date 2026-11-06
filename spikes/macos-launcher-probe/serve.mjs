import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'

const port = Number(process.argv[2])
if (![14327, 14328].includes(port)) throw new Error('Use port 14327 for dev or 14328 for the external page')
const assets = new Map([
  ['/', ['index.html', 'text/html; charset=utf-8']],
  ['/probe.js', ['probe.js', 'text/javascript; charset=utf-8']],
])
const server = createServer(async (request, response) => {
  const asset = assets.get(request.url)
  if (request.method !== 'GET' || !asset) {
    response.writeHead(404).end()
    return
  }
  try {
    const content = await readFile(new URL('./web/' + asset[0], import.meta.url))
    response.writeHead(200, { 'Content-Type': asset[1], 'Cache-Control': 'no-store' }).end(content)
  } catch {
    response.writeHead(500).end()
  }
})
server.listen(port, '127.0.0.1', () => console.log('Serving fixture at http://127.0.0.1:' + port))
