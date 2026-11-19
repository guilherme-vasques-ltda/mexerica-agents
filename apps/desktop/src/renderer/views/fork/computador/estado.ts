import type { Bot, ScreenState } from '../../../../shared/models'
import { ptBR } from '../../../lib/fork-i18n/pt-BR.ts'

/**
 * D240 (bot-dentro-02): se o bot pode "Usar o computador", pelo estado da tela que o computador
 * onde os bots trabalham informa. "Tela usável" é `screen.computerUse`, a mesma condição com que
 * ele aceita ou recusa cada chamada da ferramenta (Windows sempre; macOS e Linux com a tela ligada).
 * `connected/capture/input` só viram dica de permissão: nunca escondem o controle.
 */
export type EstadoDoComputador = 'disponivel' | 'ligadoSemTela' | 'semTela'

type TelaDoComputador = Pick<ScreenState, 'computerUse' | 'platform' | 'enabled' | 'connected' | 'capture' | 'input'>

export function estadoDoComputador(screen: TelaDoComputador | null | undefined, bot: Pick<Bot, 'computer'>): EstadoDoComputador {
  if (screen?.computerUse) return 'disponivel'
  // Um bot ligado continua recebendo a ferramenta: o interruptor fica para poder desligar.
  return bot.computer ? 'ligadoSemTela' : 'semTela'
}

// D454 (fatia 4): as frases estão no catálogo (`ajustes.usar.*`); aqui, o texto pt-BR delas e as chaves.
export const TEXTO_DISPONIVEL = ptBR['ajustes.usar.detalhe']
export const TEXTO_PERMISSAO = ptBR['ajustes.usar.libere']
export const TEXTO_LIGADO_SEM_TELA = ptBR['ajustes.usar.ligadoSemTela']
export const TEXTO_SEM_TELA = ptBR['ajustes.usar.semTela']
type ChaveDeAjuda = 'ajustes.usar.detalhe' | 'ajustes.usar.libere' | 'ajustes.usar.ligadoSemTela' | 'ajustes.usar.semTela'

export interface ControleDoComputador {
  estado: EstadoDoComputador
  mostraInterruptor: boolean
  /** Desconectado, o controle fica em leitura. */
  desabilitado: boolean
  ajuda: string
  /** As frases da ajuda, para a tela mostrar no idioma escolhido (cada uma é uma frase inteira). */
  ajudaChaves: readonly ChaveDeAjuda[]
}

export function controleDoComputador(screen: TelaDoComputador | null | undefined, bot: Pick<Bot, 'computer'>, online: boolean): ControleDoComputador {
  const estado = estadoDoComputador(screen, bot)
  const semPermissao = !!screen && screen.platform !== 'windows' && !(screen.enabled && screen.connected && screen.capture && screen.input)
  const ajudaChaves: ChaveDeAjuda[] = estado === 'disponivel' ? (semPermissao ? ['ajustes.usar.detalhe', 'ajustes.usar.libere'] : ['ajustes.usar.detalhe'])
    : estado === 'ligadoSemTela' ? ['ajustes.usar.ligadoSemTela'] : ['ajustes.usar.semTela']
  const ajuda = ajudaChaves.map((c) => ptBR[c]).join(' ')
  return { estado, mostraInterruptor: estado !== 'semTela', desabilitado: !online, ajuda, ajudaChaves }
}

/** O mesmo critério do editor: só com o computador conectado e na versão certa. */
export function podeMudarComputador(store: { connection: { kind: string }; mismatch: unknown }): boolean {
  return store.connection.kind === 'online' && !store.mismatch
}

/**
 * Envia só `{computer}` e espera a resposta; o espelho muda pelo evento do bot, nunca antes.
 * Devolve o texto da recusa (para mostrar junto ao controle) ou `null`.
 */
export async function mudarComputador(salvar: (patch: { computer: boolean }) => Promise<unknown>, computer: boolean): Promise<string | null> {
  try {
    await salvar({ computer })
    return null
  } catch (error) {
    return error instanceof Error ? error.message : String(error)
  }
}

/**
 * Depois que o Hub confirma o desligar sem tela, o interruptor sai e o foco iria para o corpo da
 * página: leva-o à linha que ficou ("Ver seus computadores"). Não tira o foco de outro lugar.
 */
export function moverFocoAoDesligar(antes: EstadoDoComputador, depois: EstadoDoComputador, desligouAqui: boolean, focoPerdido: boolean): boolean {
  return desligouAqui && focoPerdido && antes === 'ligadoSemTela' && depois === 'semTela'
}
