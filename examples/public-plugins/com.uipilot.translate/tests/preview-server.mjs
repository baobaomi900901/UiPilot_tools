import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'

const dist = new URL('../package/dist/', import.meta.url)
const types = { html: 'text/html; charset=utf-8', css: 'text/css; charset=utf-8', js: 'text/javascript; charset=utf-8' }
const allowed = new Set(['panel.html', 'panel.css', 'panel.js', 'panel-model.js', 'panel-capabilities.js', 'languages.js'])
const port = Number(process.env.TRANSLATE_PREVIEW_PORT || 14348)
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://127.0.0.1')
  const name = url.pathname.slice(1)
  try {
    let body
    if (!name) body = await readFile(new URL('preview.html', import.meta.url), 'utf8')
    else if (name === 'preview-host.js') body = await readFile(new URL(name, import.meta.url), 'utf8')
    else if (allowed.has(name)) {
      body = await readFile(new URL(name, dist), 'utf8')
      if (name === 'panel.html') body = body.replace('<script type="module" src="./panel.js">', '<script src="./preview-host.js"></script><script type="module" src="./panel.js">')
    } else { response.writeHead(404); response.end(); return }
    response.writeHead(200, { 'Content-Type': types[(name || 'index.html').split('.').pop()], 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'none'; connect-src 'none'" })
    response.end(body)
  } catch { response.writeHead(500); response.end('Preview asset unavailable') }
})
server.listen(port, '127.0.0.1', () => process.stdout.write(`Offline fake Host: http://127.0.0.1:${port}/?theme=light&layout=wide&state=success\n`))
