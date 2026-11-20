import { request, type IncomingMessage } from 'node:http'

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
        Authorization: `Bearer ${token}`,
        ...(body === null ? { Accept: 'text/event-stream' } : {
          'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body),
        }),
      },
    }, resolve)
    req.on('error', reject)
    req.setTimeout(45_000, () => req.destroy())
    req.end(body ?? undefined)
  })
}

export async function readOpenResponse(response: IncomingMessage) {
  response.setEncoding('utf8')
  let text = ''
  for await (const chunk of response) text += chunk as string
  return text
}
