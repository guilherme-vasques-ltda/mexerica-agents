// Mission Board na faixa: quando o ponto acende e o que conta. Regra pura, fora do SpaceRail, para o redesenho
// da faixa trocar só o visual. Usa só o que o renderer já tem: o estado dos bots que o Hub da faixa espelha (o mesmo
// da bandeja), sem RPC, campo novo ou polling. Sem dado (sem conexão, num space, sem Pessoal): null, nunca zero.

import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import { isolado } from '../../../../shared/bidi.ts'
import type { AttentionQueue } from '../../../../shared/fork-torre-esperando.ts'

export interface BadgeBot { status: string; kind?: string; hidden?: boolean; spaceId?: string }

export interface BadgeSource {
  /** Link vivo com o Hub da faixa. */
  online: boolean
  /** A faixa está no Pessoal (o Mission Board abriria nele). */
  inPessoal: boolean
  personalSpaceId: string | null
  bots: Iterable<BadgeBot>
}

/** Quantos bots do Pessoal estão em Precisa de você; null quando não há dado para dizer. */
export function waitingYouCount(s: BadgeSource): number | null {
  if (!s.online || !s.inPessoal || !s.personalSpaceId) return null
  let n = 0
  for (const b of s.bots) {
    if (b.kind === 'group' || b.hidden) continue
    if (b.spaceId && b.spaceId !== s.personalSpaceId) continue
    if (b.status === 'needsInput') n++
  }
  return n
}

export function attentionCount(queue: Pick<AttentionQueue, 'items'> | null): number | null {
  return queue?.items.length ?? null
}

/** O rótulo do botão: o nome sempre; a contagem só quando há quem espere. */
export function railLabel(n: number | null, idioma: AppLanguage): string {
  // O nome é o escolhido pelo dono (docs/features/mission-board.md), numa chave própria para trocar num lugar só.
  const nome = traduzir(idioma, 'torre.nome')
  // Na frase, o nome vai isolado (FSI…PDI): em árabe, o nome latino não troca a ordem da frase.
  return n ? traduzir(idioma, 'torre.rail.esperando', { nome: isolado(nome), n }) : nome
}
