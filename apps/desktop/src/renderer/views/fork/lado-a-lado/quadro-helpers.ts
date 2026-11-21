import {
  mesmaRefLadoALado,
  pertenceAoEscopo,
  type LadoALadoEscopo,
  type LadoALadoEscopoEstado,
  type LadoALadoPainel,
  type LadoALadoRef,
} from '../../../../shared/fork-lado-a-lado.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

export interface SessaoCandidata {
  id: string
  title: string
}

export interface CandidatoFonte<TBot = unknown> {
  ref: LadoALadoRef
  botId: string
  botName: string
  bot: TBot
  sessoesAbertas: readonly SessaoCandidata[]
}

export interface OpcaoConversa<TBot = unknown> {
  chave: string
  ref: LadoALadoRef
  botId: string
  botName: string
  bot: TBot
  sessionId: string | null
  sessionTitle: string | null
  principal: boolean
  temExtras: boolean
}

export type AcaoSelecaoBarra =
  | { kind: 'focar'; panelId: string }
  | { kind: 'adicionar'; ref: LadoALadoRef }
  | { kind: 'trocar'; panelId: string; ref: LadoALadoRef }
  | { kind: 'nenhuma' }

/** Desdobra cada conversa disponível em entrada principal + sessões extras abertas. */
export function opcoesDeConversa<TBot>(fontes: readonly CandidatoFonte<TBot>[]): OpcaoConversa<TBot>[] {
  const opcoes: OpcaoConversa<TBot>[] = []
  for (const fonte of fontes) {
    const temExtras = fonte.sessoesAbertas.length > 0
    const base = `${fonte.ref.computerId}/${fonte.ref.spaceId ?? ''}/${fonte.botId}`
    opcoes.push({
      chave: `${base}:principal`,
      ref: fonte.ref,
      botId: fonte.botId,
      botName: fonte.botName,
      bot: fonte.bot,
      sessionId: null,
      sessionTitle: null,
      principal: true,
      temExtras,
    })
    for (const sessao of fonte.sessoesAbertas) {
      opcoes.push({
        chave: `${base}:sessao:${sessao.id}`,
        ref: fonte.ref,
        botId: fonte.botId,
        botName: fonte.botName,
        bot: fonte.bot,
        sessionId: sessao.id,
        sessionTitle: sessao.title,
        principal: false,
        temExtras: true,
      })
    }
  }
  return opcoes
}

/** Conversas candidatas para o cartão de convite quando só há um painel aberto. */
export function opcoesDeConviteVazio<TBot>(
  fontes: readonly CandidatoFonte<TBot>[],
  paineis: readonly LadoALadoPainel[],
  limite = 6,
): CandidatoFonte<TBot>[] {
  return fontes
    .filter((fonte) => !paineis.some((p) => mesmaRefLadoALado(p.ref, fonte.ref)))
    .slice(0, Math.max(0, limite))
}

/** Informa se o par conversa + sessão já está aberto em algum painel do quadro. */
export function painelContemOpcao(
  paineis: readonly LadoALadoPainel[],
  ref: LadoALadoRef,
  sessionId: string | null,
): boolean {
  return paineis.some((p) => mesmaRefLadoALado(p.ref, ref) && p.sessionId === sessionId)
}

/** Decide o que fazer quando o usuário escolhe uma conversa na barra lateral com o quadro ativo. */
export function acaoSelecaoBarraLateral(
  estado: LadoALadoEscopoEstado,
  ref: LadoALadoRef | null,
  escopo: LadoALadoEscopo | null,
): AcaoSelecaoBarra {
  if (!estado.ativo || !ref || !pertenceAoEscopo(ref, escopo)) {
    return { kind: 'nenhuma' }
  }
  const focado = estado.paineis.find((p) => p.id === estado.focoId) ?? null
  if (focado && mesmaRefLadoALado(focado.ref, ref)) {
    return { kind: 'focar', panelId: focado.id }
  }
  const existente = estado.paineis.find((p) => mesmaRefLadoALado(p.ref, ref))
  if (existente) {
    return { kind: 'focar', panelId: existente.id }
  }
  if (estado.paineis.length <= 1) {
    return { kind: 'adicionar', ref }
  }
  const alvo = focado ?? estado.paineis[estado.paineis.length - 1] ?? estado.paineis[0]
  if (!alvo) {
    return { kind: 'nenhuma' }
  }
  return { kind: 'trocar', panelId: alvo.id, ref }
}

/** Rótulo acessível do painel, incluindo a sessão quando uma extra estiver aberta. */
export function rotuloDoPainel(
  botName: string,
  sessionId: string | null,
  sessionLabel: string | null,
  t: Tradutor,
): string {
  if (sessionId !== null && sessionLabel) {
    return t('conversa.ladoALado.painelDeSessao', { nome: botName, sessao: sessionLabel })
  }
  return t('conversa.ladoALado.painelDe', { nome: botName })
}

/** Conversa que deve assumir a seleção principal ao sair do quadro. */
export function focoAposFechar(estado: LadoALadoEscopoEstado): LadoALadoRef | null {
  return (estado.paineis.find((p) => p.id === estado.focoId) ?? estado.paineis[0])?.ref ?? null
}

/** Mensagem curta de leitor de tela quando entra ou sai um painel. */
export function anuncioMudancaPaineis(
  anteriores: readonly LadoALadoPainel[],
  atuais: readonly LadoALadoPainel[],
  nomePorRef: (ref: LadoALadoRef) => string,
  t: Tradutor,
): string {
  const antesIds = new Set(anteriores.map((p) => p.id))
  const depoisIds = new Set(atuais.map((p) => p.id))
  const adicionado = atuais.find((p) => !antesIds.has(p.id))
  if (adicionado) {
    return t('conversa.ladoALado.anuncioAdicionada', { nome: nomePorRef(adicionado.ref) })
  }
  const removido = anteriores.find((p) => !depoisIds.has(p.id))
  if (removido && atuais.length > 0) {
    return t('conversa.ladoALado.anuncioFechada', { nome: nomePorRef(removido.ref) })
  }
  return ''
}
