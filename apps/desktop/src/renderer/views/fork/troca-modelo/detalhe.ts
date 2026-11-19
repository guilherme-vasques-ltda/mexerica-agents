// DON-907 C9a (D375): a linha de troca de modelo fala em texto leigo; o detalhe técnico fica no balão.
// Contrato do Hub: `modelSwitch` na entrada de aviso (host/src/agent/bot/fork_troca_motor.rs). Textos: `conversa.troca.*`.
import type { ModelSwitch } from '../../../../shared/models.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

/** A linha: "Modelo trocado de X para Y. …" com os nomes do Hub; sem os nomes, o texto que o Hub gravou. */
export function linhaTroca(t: Tradutor, s: ModelSwitch, gravado: string): string {
  const de = s.from?.label, para = s.to?.label
  return de && para ? t('conversa.troca.linha', { de, para }) : gravado
}

/** O texto do balão da linha de troca. */
export function detalheTroca(t: Tradutor, idioma: AppLanguage, s: ModelSwitch): string {
  const handoff = s.carried === 'handoff'
  const how = t(handoff ? 'conversa.troca.balao.passagem' : 'conversa.troca.balao.sessao')
  if (typeof s.resentTokens !== 'number') return `${how} ${t('conversa.troca.balao.enviando')}`
  const n = formatarNumero(idioma, s.resentTokens, { notation: 'compact', maximumFractionDigits: 0 })
  return `${how} ${t(handoff ? 'conversa.troca.balao.enviados' : 'conversa.troca.balao.reenviados', { n })}`
}
