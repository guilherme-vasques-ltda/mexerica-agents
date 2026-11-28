// Código novo do fork (ver MODIFICACOES.md).
import { Agent, request, type IncomingMessage } from 'node:http'
import type { Socket } from 'node:net'
import { consultaProvada, reciboConfere, type LocalHealth } from './fork-local-health'

/**
 * A conexão provada: antes de a credencial (bearer) sair, a MESMA conexão TCP prova que quem
 * responde tem o token — `/health` com prova primeiro, a chamada com a credencial logo depois,
 * na mesma conexão. Uma conexão estabelecida não troca de dono no meio do caminho, então não
 * existe janela para outro listener ocupar a porta entre a prova e a credencial. Cada rodada
 * usa um agente próprio com uma conexão só, então chamadas simultâneas (e o stream de eventos,
 * que fica aberto) não se atrapalham: cada uma prova a sua. Se a conexão cair e o agente abrir
 * outra, o pedido com a credencial é destruído antes de qualquer byte sair — um listener que
 * ocupou o lugar nunca lê nada.
 */

const agentes = new Set<Agent>()

export class ProvaRecusada extends Error {}

/** Um pedido no agente da rodada. `errado` marca a conexão que não é a provada. */
type Pedido = { method: string; headers: Record<string, string>; body?: string }
type Destino = string | { socketPath: string }

function pedir(agente: Agent, destino: Destino, path: string, init: Pedido, errado: (socket: Socket) => boolean, timeoutMs: number, signal?: AbortSignal): Promise<{ res: IncomingMessage; socket: Socket }> {
  return new Promise((resolve, reject) => {
    const alvo = typeof destino === 'string' ? new URL(`${destino}${path}`) : null
    const req = request(
      {
        hostname: alvo ? alvo.hostname.startsWith('[') ? alvo.hostname.slice(1, -1) : alvo.hostname : 'localhost',
        port: alvo ? Number(alvo.port) : undefined,
        socketPath: typeof destino === 'string' ? undefined : destino.socketPath,
        path: alvo ? `${alvo.pathname}${alvo.search}` : path,
        method: init.method,
        headers: init.headers,
        agent: agente,
        signal,
      },
      (res) => {
        const socket = res.socket
        if (errado(socket)) {
          res.destroy()
          reject(new ProvaRecusada('a credencial não sai por uma conexão que não é a provada')) // textos-ok: erro técnico interno; quem chama converte em mensagem do catálogo, este texto nunca aparece
          return
        }
        resolve({ res, socket })
      },
    )
    req.on('socket', (socket) => {
      // O agente escolheu outra conexão (a provada morreu): nada sai por ela. Destruir aqui é
      // antes de qualquer byte chegar ao servidor — um listener que ocupou o lugar nunca lê nada.
      if (errado(socket)) req.destroy(new ProvaRecusada('a credencial não sai por uma conexão que não é a provada')) // textos-ok: erro técnico interno; quem chama converte em mensagem do catálogo, este texto nunca aparece
    })
    req.on('error', reject)
    req.setTimeout(timeoutMs, () => req.destroy(new Error('timeout')))
    req.end(init.body)
  })
}

function texto(res: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    res.setEncoding('utf8')
    let out = ''
    res.on('data', (parte) => (out += parte))
    res.on('end', () => resolve(out))
    res.on('error', reject)
  })
}

/**
 * Uma rodada provada: a prova sai sem credencial nenhuma; conferido o recibo, o pedido segue
 * pela mesma conexão. O agente da rodada vive até o corpo terminar ou a resposta fechar.
 */
async function provar(agente: Agent, destino: Destino, token: string, timeoutMs: number, signal?: AbortSignal) {
  const prova = consultaProvada(token)
  const saude = await pedir(agente, destino, `/health${prova.query}`, { method: 'GET', headers: {} }, () => false, timeoutMs, signal)
  if (saude.res.statusCode !== 200) {
    saude.res.resume()
    throw new ProvaRecusada(`a prova foi recusada (${saude.res.statusCode})`)
  }
  const lido: unknown = JSON.parse(await texto(saude.res))
  const health = lido !== null && typeof lido === 'object' ? lido as LocalHealth : null
  if (!health || !reciboConfere(token, prova.nonce, health)) throw new ProvaRecusada()
  return { health, socket: saude.socket }
}

export async function provarSocket(socketPath: string, token: string, signal: AbortSignal): Promise<LocalHealth> {
  const destino = { socketPath }
  const agente = new Agent({ keepAlive: true, maxSockets: 1 })
  try {
    const { health } = await provar(agente, destino, token, 2_000, signal)
    return health
  } finally {
    agente.destroy()
  }
}

async function umaVez(destino: Destino, path: string, token: string, pedido: Pedido, timeoutMs: number, signal?: AbortSignal, atual?: () => boolean): Promise<IncomingMessage> {
  signal?.throwIfAborted()
  const agente = new Agent({ keepAlive: true, maxSockets: 1 })
  agentes.add(agente)
  const fechar = () => {
    if (agentes.delete(agente)) agente.destroy()
  }
  try {
    const saude = await provar(agente, destino, token, timeoutMs, signal)
    signal?.throwIfAborted()
    if (atual && !atual()) throw new ProvaRecusada()
    const { res } = await pedir(agente, destino, path, pedido, (socket) => socket !== saude.socket, timeoutMs, signal)
    res.once('end', fechar)
    res.once('close', fechar)
    res.once('error', fechar)
    return res
  } catch (erro) {
    fechar()
    throw erro
  }
}

/**
 * POST com bearer no loopback: a prova e a credencial passam pela mesma conexão, uma atrás da
 * outra. Quem chama deve consumir ou destruir a resposta para fechar a rodada.
 */
export function postProvado(url: Destino, path: string, token: string, body: string, timeoutMs: number, options?: { signal?: AbortSignal; atual?: () => boolean }): Promise<IncomingMessage> {
  const prazo = AbortSignal.timeout(timeoutMs)
  const signal = options?.signal ? AbortSignal.any([options.signal, prazo]) : prazo
  return umaVez(url, path, token, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', 'Content-Length': String(Buffer.byteLength(body)) },
    body,
  }, timeoutMs, signal, options?.atual)
}

/**
 * GET de stream com bearer no loopback: a prova e a abertura do stream passam pela mesma conexão.
 * O stream fica aberto nessa conexão o tempo que quiser — o agente da rodada vive com ele, sem
 * segurar nem atrapalhar as outras chamadas, que provam as conexões delas.
 */
export function streamProvado(url: Destino, path: string, token: string, signal: AbortSignal, timeoutMs: number, atual?: () => boolean): Promise<IncomingMessage> {
  return umaVez(url, path, token, {
    method: 'GET', headers: { Authorization: `Bearer ${token}`, Accept: 'text/event-stream' },
  }, timeoutMs, signal, atual)
}

/** Para testes: destrói os agentes de todas as rodadas vivas. */
export async function fecharProvedLoopback() {
  for (const agente of agentes) agente.destroy()
  agentes.clear()
}
