// Entrar com OpenRouter (Conexões de IA): o Hub prepara o login PKCE no navegador e o retorno chega ao ouvinte do
// app (main/fork-oauth-loopback.ts); aqui só a regra da tela, pura e testável — a view não faz regra de negócio.
// A chave trocada nunca passa por aqui: o Hub a guarda no cofre e devolve só o mesmo «Acesso» de Testar acesso.
import type { HostClient } from '../../../client/host-client'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { lerRetornoLogin } from '../meus-mcp/retorno-login.ts'
import type { Acesso } from './api-direta'

/** O ouvinte de retorno do app (main/fork-oauth-loopback-ipc.ts), como o renderer o vê. */
export interface PonteDoOuvinte {
  abrir(): Promise<{ porta: number; tentativa: number } | null>
  esperar(tentativa: number, url: string): Promise<string>
  responder(tentativa: number, resultado: { ok: boolean; nome?: string; detalhe?: string }): void
  fechar(tentativa: number): void
}

/** O login não chegou ao fim: a pessoa desistiu, o prazo acabou ou o fluxo não conseguiu continuar. */
export class FalhaOpenRouter extends Error {
  readonly tipo: 'cancelado' | 'tempo' | 'falha'
  constructor(tipo: FalhaOpenRouter['tipo']) {
    super(tipo) // textos-ok: código técnico interno, nunca mostrado
    this.tipo = tipo
  }
}

/** A frase de cada falha do login, para o bloco de resultado da linha. */
export const fraseDaFalha = (tipo: FalhaOpenRouter['tipo']): ChaveTexto =>
  tipo === 'cancelado' ? 'contas.apiDireta.openrouter.erro.cancelado'
    : tipo === 'tempo' ? 'contas.apiDireta.openrouter.erro.tempo'
      : 'contas.apiDireta.openrouter.erro.falha'

const PRAZO_MS = 60_000
/** O ouvinte corta a espera em 10 minutos; aqui corta um pouco antes, para a frase ser a de tempo e não a genérica. */
const ESPERA_MAX_MS = 10 * 60_000 - 2_000

/**
 * O login inteiro: liga o ouvinte do app, pede a URL ao Hub, espera o navegador voltar, entrega o retorno ao Hub
 * (que confere o state, consome a tentativa, troca o código e guarda a chave no cofre) e mostra o desfecho ao
 * navegador. `cancelar` desiste enquanto o navegador não voltou: desliga o ouvinte e a espera termina como «cancelado»;
 * depois do retorno (`aoConcluir`) ele não faz mais nada, porque o Hub já está gravando a chave.
 */
export function entrarComOpenRouter(client: HostClient, ouvinte: PonteDoOuvinte, esperaMs = ESPERA_MAX_MS, aoConcluir?: () => void) {
  let tentativa: number | null = null
  let cancelado = false
  let concluindo = false
  const fim = (async (): Promise<Acesso> => {
    const aberto = await ouvinte.abrir().catch(() => null)
    if (!aberto) throw new FalhaOpenRouter('falha')
    tentativa = aberto.tentativa
    try {
      if (cancelado) throw new FalhaOpenRouter('cancelado')
      let url: string
      try { url = (await client.call<{ url: string }>('openrouterEntrar', { loopbackPort: aberto.porta }, 15_000)).url }
      catch { throw new FalhaOpenRouter('falha') }
      if (cancelado) throw new FalhaOpenRouter('cancelado')
      let temporizador: ReturnType<typeof setTimeout> | undefined
      const espera = ouvinte.esperar(aberto.tentativa, url)
      const prazo = new Promise<never>((_, recusar) => { temporizador = setTimeout(() => recusar(new FalhaOpenRouter('tempo')), esperaMs) })
      // Quem perder a corrida rejeita depois, sem ninguém esperando: não pode derrubar o processo.
      espera.catch(() => {})
      prazo.catch(() => {})
      let volta: string
      try {
        volta = await Promise.race([espera, prazo])
      } catch (e) {
        if (e instanceof FalhaOpenRouter) throw e
        throw new FalhaOpenRouter(cancelado ? 'cancelado' : 'falha')
      } finally {
        clearTimeout(temporizador)
      }
      let retorno: ReturnType<typeof lerRetornoLogin>
      try { retorno = lerRetornoLogin(volta) } catch { throw new FalhaOpenRouter('falha') }
      // Daqui em diante o Hub troca o código e grava a chave: não há mais o que cancelar.
      concluindo = true
      aoConcluir?.()
      let acesso: Acesso
      try {
        acesso = await client.call<Acesso>('openrouterEntrarConcluir',
          retorno.code !== null ? { state: retorno.state, code: retorno.code } : { state: retorno.state, erro: retorno.error ?? '' }, PRAZO_MS)
      } catch { throw new FalhaOpenRouter('falha') }
      ouvinte.responder(aberto.tentativa, acesso.ok ? { ok: true, nome: 'OpenRouter' } : { ok: false })
      if (!acesso.ok && (acesso.erro as string) === 'entrada_negada') throw new FalhaOpenRouter('cancelado')
      return acesso
    } finally {
      ouvinte.fechar(aberto.tentativa)
    }
  })()
  const cancelar = () => {
    if (concluindo) return
    cancelado = true
    if (tentativa !== null) ouvinte.fechar(tentativa)
  }
  return { fim, cancelar }
}
