// DON-917 (fatia 9d): hora e "há quanto tempo" do Mission Board no idioma da tela. Puro (os testes do Node o usam).
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { ltr } from '../../../../shared/bidi.ts'
import type { Ago } from '../../../../shared/fork-torre-pedidos.ts'
import { localeDe, traduzir } from '../../../lib/fork-i18n/index.ts'

/** A hora no formato do idioma, isolada da esquerda para a direita (em árabe não troca a ordem da frase). */
export const hora = (ms: number, idioma: AppLanguage) => ltr(new Date(ms).toLocaleTimeString(localeDe(idioma), { hour: '2-digit', minute: '2-digit' }))

/** Há quanto tempo, pelo `Intl.RelativeTimeFormat` do idioma; menos de 1 minuto vem do catálogo. */
export function haQuanto(a: Ago, idioma: AppLanguage) {
  if (a.unit === 'now') return traduzir(idioma, 'torre.pedidos.agoraHaPouco')
  return new Intl.RelativeTimeFormat(localeDe(idioma), { numeric: 'always', style: 'short' }).format(-a.n, a.unit)
}
