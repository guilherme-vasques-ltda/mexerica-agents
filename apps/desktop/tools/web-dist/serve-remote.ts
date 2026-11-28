import { createServer } from 'node:http'
import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { extname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { pipeline } from 'node:stream/promises'

const mime: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.gif': 'image/gif',
  '.woff2': 'font/woff2',
  '.mp3': 'audio/mpeg',
  '.wasm': 'application/wasm',
}

export function serveRemoteWeb(dist: string) {
  return createServer(async (request, response) => {
    response.setHeader('X-Content-Type-Options', 'nosniff')
    response.setHeader('Referrer-Policy', 'no-referrer')
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      response.writeHead(405, { Allow: 'GET, HEAD' }).end()
      return
    }
    const pathname = request.url?.split('?', 1)[0]
    const path = pathname === '/' || pathname === '/index.html' ? 'index.html'
      : pathname === '/renderer.html' ? 'renderer.html'
      : pathname && /^\/assets\/[a-z0-9_.-]+\.[a-z0-9]+$/i.test(pathname) ? pathname.slice(1) : null
    const type = path ? mime[extname(path)] : undefined
    if (!path || !type) { response.writeHead(404).end(); return }
    const renderer = path === 'renderer.html'
    response.setHeader('Content-Security-Policy', renderer ? "frame-ancestors 'self'" : "frame-ancestors 'none'")
    response.setHeader('X-Frame-Options', renderer ? 'SAMEORIGIN' : 'DENY')
    response.setHeader('Cache-Control', path.endsWith('.html') ? 'no-store' : 'public, max-age=3600')
    const file = join(dist, path)
    const info = await stat(file).catch(() => null)
    if (!info?.isFile()) { response.writeHead(404).end(); return }
    response.writeHead(200, { 'Content-Type': type, 'Content-Length': info.size })
    if (request.method === 'HEAD') { response.end(); return }
    await pipeline(createReadStream(file), response).catch(() => response.destroy())
  })
}

if (import.meta.main) {
  const port = Number(process.env.WEB_REMOTE_PORT ?? '5173')
  if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error('Invalid WEB_REMOTE_PORT')
  const dist = fileURLToPath(new URL('../../dist/web-remote/', import.meta.url))
  if (!(await stat(join(dist, 'index.html'))).isFile()) throw new Error('Run npm run build:web:remote first')
  serveRemoteWeb(dist).listen(port, '127.0.0.1')
}
