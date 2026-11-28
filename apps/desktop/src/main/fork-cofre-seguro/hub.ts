// MX-209: como o main chega ao MESMO Hub do cartão, sem receber endereço nem token do renderer.
// O chat só diz o id do computador e do pedido. O main procura esse id entre os Hubs que ele próprio conhece:
//   - o Hub deste computador (HostController.local);
//   - um computador SSH ligado por túnel em 127.0.0.1, por uma CAPTURA com as guardas de
//     `captureLocal`/`createLocalCloudCapture` (identidade, validade e posse do listener), nunca o anexo cru.
// Conexão pela nuvem (relay) NÃO é aceita: o envio por lá é assinado e com resultado incerto (RelayOutcomeUnknown),
// e o valor não pode ir para um canal que o main não controla de ponta a ponta. Falha fechada.
import type { LocalHost } from '../../shared/ipc'
import { postProvado } from '../fork-proved-loopback'

/** O que o main precisa de um Hub para falar com ele com o segredo: endereço+token e as guardas de vida. */
export interface CapturaHub {
  baseURL: string
  token: string
  socketPath?: string
  signal?: AbortSignal
  /** A mesma conexão continua viva (processo local, túnel, perfil e a janela que a pediu). */
  current(): boolean
  /** O listener da porta ainda pertence ao processo esperado; no Hub local é sempre true. */
  listener(): Promise<boolean>
}

export interface FontesDeHub {
  local: () => (LocalHost & { socketPath?: string; signal?: AbortSignal }) | null
  /** Captura viva do túnel SSH do computador (guardas do ssh.ts), ou null se não houver. */
  ssh: (computerId: string, current: () => boolean) => CapturaHub | null
}
export type FalhaHub = { status: number }

const loopback = (url: string) => {
  try {
    const { protocol, hostname } = new URL(url)
    return protocol === 'http:' && (hostname === '127.0.0.1' || hostname === '[::1]' || hostname === 'localhost')
  } catch {
    return false
  }
}

const vivaLocal = (local: LocalHost, fontes: FontesDeHub, consumidora: () => boolean): boolean => {
  try {
    const agora = fontes.local()
    return consumidora() === true && agora === local && (agora.socketPath ? agora.signal?.aborted === false : loopback(agora.baseURL))
  } catch {
    return false
  }
}

/**
 * Resolve o computador para uma captura de Hub em loopback que o main conhece; `null` = não dá (fecha).
 * A captura fica presa à `consumidora` (a janela aberta): troca de host, túnel ou perfil a invalida.
 */
export function resolverHub(computerId: string, fontes: FontesDeHub, consumidora: () => boolean): CapturaHub | null {
  const local = fontes.local()
  if (local && local.computerId === computerId) {
    if (!vivaLocal(local, fontes, consumidora)) return null
    return {
      baseURL: local.baseURL, token: local.token, socketPath: local.socketPath, signal: local.signal,
      current: () => vivaLocal(local, fontes, consumidora),
      listener: async () => true,
    }
  }
  return fontes.ssh(computerId, consumidora)
}

/**
 * POST /api/<método> na captura resolvida. Bearer e corpo só saem pela conexão que provou o token,
 * sem seguir redirecionamento. A posse do listener e a validade são conferidas antes e depois do envio.
 * Não loga, e nenhum erro lançado leva corpo, URL com token ou resposta (só o status).
 */
export async function chamarHub(captura: CapturaHub, metodo: string, corpo: object, timeoutMs = 120_000): Promise<unknown> {
  const corrente = (): boolean => {
    try {
      return captura.current()
    } catch {
      return false
    }
  }
  // Mesmas guardas do `createLocalCloudCapture`: `current()` antes e DEPOIS de cada espera
  // (listener, resposta) — a captura pode morrer enquanto o lsof/proc responde. Falha fechada,
  // sem repetir envio de resultado incerto.
  const guarda = async (): Promise<boolean> => {
    try {
      return corrente() && (await captura.listener()) && corrente()
    } catch {
      return false
    }
  }
  if ((!captura.socketPath && !loopback(captura.baseURL)) || !(await guarda()) || !corrente()) throw { status: 0 } satisfies FalhaHub
  let resposta: Awaited<ReturnType<typeof postProvado>>
  try {
    resposta = await postProvado(captura.socketPath ? { socketPath: captura.socketPath } : captura.baseURL, `/api/${metodo}`, captura.token, JSON.stringify(corpo), timeoutMs, {
      signal: captura.signal ?? AbortSignal.timeout(timeoutMs),
      atual: corrente,
    })
  } catch {
    throw { status: 0 } satisfies FalhaHub
  }
  try {
    // O envio pode ter acontecido: se a captura morreu no meio, o resultado é incerto — falha fechada.
    if (!(await guarda())) throw { status: 0 } satisfies FalhaHub
    const status = resposta.statusCode ?? 0
    if (status < 200 || status >= 300) throw { status } satisfies FalhaHub
    let dados: unknown
    try {
      let texto = ''
      resposta.setEncoding('utf8')
      for await (const parte of resposta) texto += parte as string
      dados = JSON.parse(texto)
    } catch {
      throw { status } satisfies FalhaHub
    }
    if (!(await guarda())) throw { status: 0 } satisfies FalhaHub
    return dados
  } finally {
    resposta.destroy()
  }
}
