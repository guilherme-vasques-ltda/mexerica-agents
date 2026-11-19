// MX-209: como o main chega ao MESMO Hub do cartão, sem receber endereço nem token do renderer.
// O chat só diz o id do computador e do pedido. O main procura esse id entre os Hubs que ele próprio conhece:
//   - o Hub deste computador (HostController.local);
//   - um computador SSH ligado por túnel em 127.0.0.1, por uma CAPTURA com as guardas de
//     `captureLocal`/`createLocalCloudCapture` (identidade, validade e posse do listener), nunca o anexo cru.
// Conexão pela nuvem (relay) NÃO é aceita: o envio por lá é assinado e com resultado incerto (RelayOutcomeUnknown),
// e o valor não pode ir para um canal que o main não controla de ponta a ponta. Falha fechada.
import type { LocalHost } from '../../shared/ipc'

/** O que o main precisa de um Hub para falar com ele com o segredo: endereço+token e as guardas de vida. */
export interface CapturaHub {
  baseURL: string
  token: string
  /** A mesma conexão continua viva (processo local, túnel, perfil e a janela que a pediu). */
  current(): boolean
  /** O listener da porta ainda pertence ao processo esperado; no Hub local é sempre true. */
  listener(): Promise<boolean>
}

export interface FontesDeHub {
  local: () => LocalHost | null
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
    return consumidora() === true && agora === local && loopback(agora.baseURL)
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
    if (!loopback(local.baseURL) || !vivaLocal(local, fontes, consumidora)) return null
    return {
      baseURL: local.baseURL, token: local.token,
      current: () => vivaLocal(local, fontes, consumidora),
      listener: async () => true,
    }
  }
  return fontes.ssh(computerId, consumidora)
}

/**
 * POST /api/<método> na captura resolvida. O corpo pode carregar o valor: ele só existe aqui, na fila do fetch.
 * Mesmas guardas do `createLocalCloudCapture`: posse do listener e validade conferidas imediatamente antes
 * do envio e de novo depois; `redirect: 'error'` impede que um 307/308 reenvie o corpo a outra origem.
 * Não loga, e nenhum erro lançado leva corpo, URL com token ou resposta (só o status).
 */
export async function chamarHub(captura: CapturaHub, metodo: string, corpo: object, http: typeof fetch = fetch, timeoutMs = 120_000): Promise<unknown> {
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
  if (!(await guarda()) || !corrente()) throw { status: 0 } satisfies FalhaHub
  let resposta: Response
  try {
    resposta = await http(`${captura.baseURL}/api/${metodo}`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${captura.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(corpo),
      redirect: 'error',
      signal: AbortSignal.timeout(timeoutMs),
    })
  } catch {
    throw { status: 0 } satisfies FalhaHub
  }
  // O envio pode ter acontecido: se a captura morreu no meio, o resultado é incerto — falha fechada.
  if (!(await guarda())) throw { status: 0 } satisfies FalhaHub
  if (!resposta.ok) throw { status: resposta.status } satisfies FalhaHub
  let dados: unknown
  try {
    dados = await resposta.json()
  } catch {
    throw { status: resposta.status } satisfies FalhaHub
  }
  if (!(await guarda())) throw { status: 0 } satisfies FalhaHub
  return dados
}
