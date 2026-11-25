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
function pedir(agente: Agent, url: string, init: { method: string; headers: Record<string, string>; body?: string }, errado: (socket: Socket) => boolean, timeoutMs: number): Promise<{ res: IncomingMessage; socket: Socket }> {
  return new Promise((resolve, reject) => {
    const alvo = new URL(url)
    const req = request(
      {
        host: alvo.hostname,
        port: Number(alvo.port),
        path: `${alvo.pathname}${alvo.search}`,
        method: init.method,
        headers: init.headers,
        agent: agente,
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
 * Uma rodada provada: a prova sai sem credencial nenhuma; conferido o recibo, `enviar` manda a
 * credencial pela MESMA conexão. O agente da rodada morre com ela — a conexão provada de uma
 * rodada nunca serve para outra.
 */
async function umaVez<T>(url: string, token: string, enviar: (agente: Agent, provado: Socket) => Promise<T>, timeoutMs: number): Promise<T> {
  const agente = new Agent({ keepAlive: true, maxSockets: 1 })
  agentes.add(agente)
  try {
    const prova = consultaProvada(token)
    const saude = await pedir(agente, `${url}/health${prova.query}`, { method: 'GET', headers: {} }, () => false, timeoutMs)
    if (saude.res.statusCode !== 200) {
      saude.res.resume()
      throw new ProvaRecusada(`a prova foi recusada (${saude.res.statusCode})`)
    }
    const corpo = await texto(saude.res)
    let health: LocalHealth | null = null
    try {
      const lido: unknown = JSON.parse(corpo)
      if (lido !== null && typeof lido === 'object') health = lido as LocalHealth
    } catch { /* corpo quebrado não é recibo */ }
    if (!health || !reciboConfere(token, prova.nonce, health)) throw new ProvaRecusada('a resposta não trouxe o recibo do token') // textos-ok: erro técnico interno; quem chama converte em mensagem do catálogo, este texto nunca aparece
    return await enviar(agente, saude.socket)
  } finally {
    agentes.delete(agente)
    agente.destroy()
  }
}

/** A conexão provada da rodada é uma só: qualquer outra é recusada antes de carregar credencial. */
const soAProvada = (provado: Socket) => (socket: Socket) => socket !== provado

/**
 * POST com bearer no loopback: a prova e a credencial passam pela mesma conexão, uma atrás da
 * outra. O agente da rodada fecha com a resposta consumida — quem lê o corpo no chamador o faz
 * antes de o `finally` destruir o agente, porque o corpo vem do socket já recebido.
 */
export function postProvado(url: string, path: string, token: string, body: string, timeoutMs: number): Promise<IncomingMessage> {
  return umaVez(
    url,
    token,
    async (agente, provado) => {
      const r = await pedir(
        agente,
        `${url}${path}`,
        { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', 'Content-Length': String(Buffer.byteLength(body)) }, body },
        soAProvada(provado),
        timeoutMs,
      )
      return r.res
    },
    timeoutMs,
  )
}

/**
 * GET de stream com bearer no loopback: a prova e a abertura do stream passam pela mesma conexão.
 * O stream fica aberto nessa conexão o tempo que quiser — o agente da rodada vive com ele, sem
 * segurar nem atrapalhar as outras chamadas, que provam as conexões delas.
 */
export async function streamProvado(url: string, path: string, token: string, signal: AbortSignal, timeoutMs: number): Promise<IncomingMessage> {
  const agente = new Agent({ keepAlive: true, maxSockets: 1 })
  agentes.add(agente)
  const prova = consultaProvada(token)
  let saude: { res: IncomingMessage; socket: Socket }
  try {
    saude = await pedir(agente, `${url}/health${prova.query}`, { method: 'GET', headers: {} }, () => false, timeoutMs)
    if (saude.res.statusCode !== 200) {
      saude.res.resume()
      throw new ProvaRecusada(`a prova foi recusada (${saude.res.statusCode})`)
    }
    const corpo = await texto(saude.res)
    let health: LocalHealth | null = null
    try {
      const lido: unknown = JSON.parse(corpo)
      if (lido !== null && typeof lido === 'object') health = lido as LocalHealth
    } catch { /* corpo quebrado não é recibo */ }
    if (!health || !reciboConfere(token, prova.nonce, health)) throw new ProvaRecusada('a resposta não trouxe o recibo do token') // textos-ok: erro técnico interno; quem chama converte em mensagem do catálogo, este texto nunca aparece
    const r = await pedir(
      agente,
      `${url}${path}`,
      { method: 'GET', headers: { Authorization: `Bearer ${token}`, Accept: 'text/event-stream' } },
      soAProvada(saude.socket),
      timeoutMs,
    )
    const res = r.res
    // O agente vive com o stream: fecha quando ele termina ou aborta. O aborto destrói a resposta
    // em si — o agente não toca a conexão enquanto o stream a ocupa, então fechar só o agente
    // deixaria o stream aberto para sempre.
    const fechar = () => {
      agentes.delete(agente)
      agente.destroy()
    }
    res.on('close', fechar)
    signal.addEventListener('abort', () => {
      res.destroy()
      fechar()
    }, { once: true })
    return res
  } catch (erro) {
    agentes.delete(agente)
    agente.destroy()
    throw erro
  }
}

/** Para testes: destrói os agentes de todas as rodadas vivas. */
export async function fecharProvedLoopback() {
  for (const agente of agentes) agente.destroy()
  agentes.clear()
}
