// I-023: o desfecho de uma execução de rotina, em linguagem simples (D233). Única fonte de rótulo, tom e frase
// para a lista, a grade, o painel do bot e o leitor de tela. O Hub decide o desfecho; o app só o descreve.
// Valor desconhecido (Hub mais novo ou dado ilegível) é tratado como "sem confirmação", nunca como sucesso.
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { traduzir, type ChaveTexto } from '../../lib/fork-i18n/index.ts'

export interface RunOutcome {
  version?: number
  kind?: string
  source?: string
  reason?: string
  summary?: string | null
  visibleReply?: boolean
}

/** `ok`: ✓ (sem novidades, com resultado). `attention`: precisa do dono. `danger`: falha. */
export type OutcomeTone = 'ok' | 'attention' | 'danger' | 'neutral' | 'active'

export interface OutcomeText {
  label: string
  tone: OutcomeTone
  /** Dica e leitor de tela; vazia quando o rótulo basta. */
  sentence: string
  /** "Sem confirmação": a lista usa o estado, não o texto (que muda com o idioma). */
  unconfirmed?: true
}

const ACTIVE = ['pending', 'starting', 'running', 'recovering'] as const
type Ativo = (typeof ACTIVE)[number]
const isAtivo = (status: string): status is Ativo => (ACTIVE as readonly string[]).includes(status)

const observed = {
  timeout: 'rotinas.desfecho.tempoLimite',
  limit: 'rotinas.desfecho.limite',
  signInRequested: 'rotinas.desfecho.entrar',
  startFailed: 'rotinas.desfecho.naoComecou',
  queueFailed: 'rotinas.desfecho.naoComecou',
} as const

/** DON-917: rótulo e frase no idioma do app (`idioma` explícito: o módulo é puro e testado sem janela). */
export function runOutcomeText(status: string, outcome: RunOutcome | null | undefined, idioma: AppLanguage): OutcomeText {
  const t = (chave: ChaveTexto) => (traduzir as (idioma: AppLanguage, chave: ChaveTexto) => string)(idioma, chave)
  const said = (summary: string | null | undefined) => (summary ? traduzir(idioma, 'rotinas.desfecho.segundoBotResumo', { resumo: summary }) : t('rotinas.desfecho.segundoBot'))
  const unconfirmed = (sentence: string): OutcomeText => ({ label: t('rotinas.desfecho.semConfirmacao'), tone: 'neutral', sentence, unconfirmed: true })
  const unreadable = () => unconfirmed(t('rotinas.desfecho.ilegivel'))
  if (isAtivo(status)) return { label: t(`rotinas.estado.${status}`), tone: 'active', sentence: '' }
  if (!outcome || typeof outcome !== 'object') {
    // Execução anterior a I-023 ou Hub antigo: nunca ✓ nem "Concluída".
    if (status === 'succeeded') return { label: t('rotinas.desfecho.terminou'), tone: 'neutral', sentence: t('rotinas.desfecho.anterior') }
    if (status === 'failed') return { label: t('rotinas.estado.failed'), tone: 'danger', sentence: '' }
    if (status === 'interrupted') return { label: t('rotinas.estado.interrupted'), tone: 'danger', sentence: '' }
    if (status === 'cancelled') return { label: t('rotinas.estado.cancelled'), tone: 'neutral', sentence: '' }
    return unreadable()
  }
  // Leitura conservadora (I-023 R1): desfecho positivo ou "precisa de você" só vale se veio declarado pelo bot num
  // registro da versão conhecida e sem motivo (no schema v1 a declaração não tem `reason`); origem, versão ou motivo
  // desconhecido nunca dá ✓.
  const declared = outcome.version === 1 && outcome.source === 'declared' && outcome.reason == null
  if (!declared && (outcome.kind === 'noNews' || outcome.kind === 'reported' || outcome.kind === 'needsOwner')) return unreadable()
  switch (outcome.kind) {
    case 'noNews':
      return { label: t('rotinas.desfecho.semNovidades'), tone: 'ok', sentence: t('rotinas.desfecho.semNovidadesFrase') }
    case 'reported':
      return { label: t('rotinas.desfecho.comResultado'), tone: 'ok', sentence: said(outcome.summary) }
    case 'needsOwner':
      return { label: t('rotinas.desfecho.precisa'), tone: 'attention', sentence: outcome.summary ? traduzir(idioma, 'rotinas.desfecho.precisaResumo', { resumo: outcome.summary }) : t('rotinas.desfecho.precisaFrase') }
    case 'failed':
      if (declared) return { label: t('rotinas.desfecho.naoConseguiu'), tone: 'danger', sentence: said(outcome.summary) }
      return { label: t('rotinas.estado.failed'), tone: 'danger', sentence: t(Object.hasOwn(observed, outcome.reason ?? '') ? observed[outcome.reason as keyof typeof observed] : 'rotinas.desfecho.parou') }
    case 'unconfirmed':
      if (outcome.reason === 'interrupted') return { label: t('rotinas.estado.interrupted'), tone: 'danger', sentence: t('rotinas.desfecho.semConfirmacaoFrase') }
      if (outcome.reason === 'stopped') return { label: t('rotinas.estado.cancelled'), tone: 'neutral', sentence: t('rotinas.desfecho.semConfirmacaoFrase') }
      if (outcome.reason === 'conflict') return unconfirmed(t('rotinas.desfecho.conflito'))
      return unconfirmed(t('rotinas.desfecho.confiraConversa'))
    default:
      return unreadable()
  }
}

/** "Rótulo — frase", para leitores de tela e dicas. */
export const outcomeLabel = (text: OutcomeText) => (text.sentence ? `${text.label}. ${text.sentence}` : text.label)
