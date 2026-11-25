import { request, type IncomingMessage } from 'node:http'
import type { Readable } from 'node:stream'

export function isLoginPageURL(url: unknown): url is string {
  return url === 'https://auth.openai.com/codex/device'
}

export function loginPageDeadline(result: unknown, now: number): number {
  if (!result || typeof result !== 'object') return 0
  const { phase, expiresAt, challenge } = result as Record<string, unknown>
  if (phase !== 'awaiting_authorization' || typeof expiresAt !== 'number' ||
      !Number.isSafeInteger(expiresAt) || expiresAt <= now || expiresAt > now + 600_000 ||
      !challenge || typeof challenge !== 'object') return 0
  const { kind, url } = challenge as Record<string, unknown>
  return kind === 'device_code' && isLoginPageURL(url) ? expiresAt : 0
}

export function openApiPath(method: unknown) {
  return typeof method === 'string' && /^[A-Za-z][A-Za-z0-9]{0,79}(?![\s\S])/.test(method) ? `/api/${method}` : null
}

export function openStreamPath(origin: string, url: unknown) {
  if (typeof url !== 'string' || !url.startsWith(origin)) return null
  const path = url.slice(origin.length)
  return /^(?:\/events(?:\?[^\s#]*)?|\/term\/[A-Za-z0-9_-]{1,120})(?![\s\S])/.test(path) ? path : null
}

export function requestOpenHub(socketPath: string, path: string, token: string, body: string | null, signal: AbortSignal) {
  return new Promise<IncomingMessage>((resolve, reject) => {
    const req = request({
      socketPath, path, signal, agent: false,
      method: body === null ? 'GET' : 'POST',
      headers: {
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(body === null ? { Accept: 'text/event-stream' } : {
          'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body),
        }),
      },
    }, resolve)
    req.on('error', reject)
    if (body === null) req.setTimeout(45_000, () => req.destroy())
    req.end(body ?? undefined)
  })
}

export async function readOpenResponse(response: IncomingMessage) {
  response.setEncoding('utf8')
  let text = ''
  for await (const chunk of response) text += chunk as string
  return text
}

export async function* readOpenEventData(response: Readable) {
  response.setEncoding('utf8')
  let buffer = ''
  for await (const chunk of response) {
    buffer += chunk as string
    let end: number
    while ((end = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, end).replace(/\r$/, '')
      buffer = buffer.slice(end + 1)
      if (line.startsWith('data:')) yield line.slice(5).trimStart()
    }
  }
}
