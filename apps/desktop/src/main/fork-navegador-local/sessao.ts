import { randomUUID } from 'node:crypto'
import { guardasFixture, permiteUrlFixture } from './politica.ts'

export const NAVEGADOR_LOCAL_HABILITADO: false = false
export const LIMITE_CAPTURA_BYTES = 256 * 1024

export function abrirNavegadorLocal() {
  return {
    estado: 'indisponivel',
    motivo: NAVEGADOR_LOCAL_HABILITADO ? 'sem-adaptador' : 'desligado',
  } as const
}

export type Escopo = Readonly<{ workspace: string; bot: string; geracao: string }>
export type Vinculo = Escopo & Readonly<{ instancia: string }>
type Comando = 'abrir' | 'ler' | 'parar'

export interface GateHubLocal {
  autoriza(comando: Comando, vinculo: Vinculo, pedido: unknown): boolean
}

export interface SuperficieFixture {
  capturar(sinal: AbortSignal): Promise<Uint8Array>
  fechar(): Promise<boolean>
  limparArmazenamento(): Promise<boolean>
  limparCache(): Promise<boolean>
}

type CriarSuperficieFixture = (config: Readonly<{
  url: string
  particao: string
  guardas: ReturnType<typeof guardasFixture>
}>) => SuperficieFixture

type Sessao = {
  vinculo: Vinculo
  cancelamento: AbortController
  superficie?: SuperficieFixture
  limpeza?: Promise<boolean>
  lendo: boolean
}

const negado = () => ({ estado: 'negado' } as const)
const indisponivel = () => ({ estado: 'indisponivel' } as const)

export function criarCicloFixture(criarSuperficie: CriarSuperficieFixture, gate?: GateHubLocal) {
  let atual: Sessao | undefined
  let encerrado = false

  function autoriza(comando: Comando, vinculo: Vinculo, pedido: unknown) {
    try {
      return gate?.autoriza(comando, vinculo, pedido) === true
    } catch {
      void invalidar()
      return false
    }
  }

  function ativa(sessao: Sessao) {
    return !encerrado && atual === sessao && !sessao.cancelamento.signal.aborted
  }

  function encontrar(vinculo: Vinculo) {
    const sessao = atual
    if (!sessao) return undefined
    const salvo = sessao.vinculo
    return salvo.workspace === vinculo.workspace && salvo.bot === vinculo.bot &&
      salvo.geracao === vinculo.geracao && salvo.instancia === vinculo.instancia ? sessao : undefined
  }

  function descartar(sessao: Sessao) {
    if (sessao.limpeza) return sessao.limpeza
    sessao.limpeza = Promise.resolve().then(async () => {
      const superficie = sessao.superficie
      if (!superficie) return false
      const confirmar = async (acao: () => Promise<boolean>) => {
        try {
          return await acao() === true
        } catch {
          return false
        }
      }
      const fechou = await confirmar(() => superficie.fechar())
      const armazenamento = await confirmar(() => superficie.limparArmazenamento())
      const cache = await confirmar(() => superficie.limparCache())
      if (!fechou || !armazenamento || !cache) return false
      atual = undefined
      return true
    })
    sessao.cancelamento.abort()
    return sessao.limpeza
  }

  function invalidar() {
    encerrado = true
    return atual ? descartar(atual) : Promise.resolve(true)
  }

  return {
    comandos: {
      abrir(pedido: unknown, escopo: Escopo, url: string) {
        if (encerrado || atual) return indisponivel()
        if (!permiteUrlFixture('navegacao', url)) return negado()
        const vinculo = Object.freeze({
          workspace: escopo.workspace, bot: escopo.bot, geracao: escopo.geracao, instancia: randomUUID(),
        })
        if ([vinculo.workspace, vinculo.bot, vinculo.geracao].some((id) => typeof id !== 'string' || !id)) {
          return negado()
        }
        if (!autoriza('abrir', vinculo, pedido)) return negado()
        if (encerrado || atual) return indisponivel()
        const sessao: Sessao = { vinculo, cancelamento: new AbortController(), lendo: false }
        atual = sessao
        try {
          sessao.superficie = criarSuperficie(Object.freeze({
            url, particao: `navegador-local-${randomUUID()}`, guardas: guardasFixture(() => ativa(sessao)),
          }))
        } catch {
          void descartar(sessao)
          return indisponivel()
        }
        if (!ativa(sessao)) return indisponivel()
        return { estado: 'aberta', vinculo } as const
      },

      async ler(pedido: unknown, vinculo: Vinculo) {
        const sessao = encontrar(vinculo)
        if (!sessao || !ativa(sessao) || !autoriza('ler', sessao.vinculo, pedido)) return negado()
        const superficie = sessao.superficie
        if (!ativa(sessao) || !superficie || sessao.lendo) return indisponivel()
        sessao.lendo = true
        const sinal = sessao.cancelamento.signal
        let cancelar: (() => void) | undefined
        try {
          const cancelada = new Promise<undefined>((resolve) => {
            cancelar = () => resolve(undefined)
            sinal.addEventListener('abort', cancelar, { once: true })
          })
          const dados = await Promise.race([cancelada, superficie.capturar(sinal)])
          if (!ativa(sessao)) return indisponivel()
          if (!autoriza('ler', sessao.vinculo, pedido)) {
            void descartar(sessao)
            return negado()
          }
          if (!ativa(sessao) || !(dados instanceof Uint8Array) ||
              dados.byteLength === 0 || dados.byteLength > LIMITE_CAPTURA_BYTES) {
            return indisponivel()
          }
          return { estado: 'capturada', confianca: 'nao-confiavel', bytes: Uint8Array.from(dados) } as const
        } catch {
          return indisponivel()
        } finally {
          if (cancelar) sinal.removeEventListener('abort', cancelar)
          sessao.lendo = false
        }
      },

      async parar(pedido: unknown, vinculo: Vinculo) {
        const sessao = encontrar(vinculo)
        if (!sessao || !ativa(sessao) || !autoriza('parar', sessao.vinculo, pedido)) return negado()
        return await descartar(sessao) ? { estado: 'encerrada' } as const : indisponivel()
      },
    },
    ciclo: {
      revogar(vinculo: Vinculo) {
        const sessao = encontrar(vinculo)
        return sessao ? descartar(sessao) : Promise.resolve(false)
      },
      hubCaiu: invalidar,
      destruirApp: invalidar,
      mudarGeracao: invalidar,
    },
  }
}
