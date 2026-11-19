import type { MemoryNature, MemoryOrigin, MemoryWrite } from '../../../../shared/models'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarData, type ChaveTexto, type Tradutor } from '../../../lib/fork-i18n/index.ts'

// Fatia 7 (D454): os rótulos saem do catálogo (`memoria.auto.*`) no idioma de `t`, ao desenhar.

/** A data curta de uma lembrança, versão ou consolidação no formato do idioma (antes `panelDate`, sempre pt-BR). */
export const dataMemoria = (idioma: AppLanguage, at: number) => formatarData(idioma, at, { day: 'numeric', month: 'short', year: 'numeric' })

/** O nome do bot que inferiu, com o artigo da marca quando é o Berga (D480). */
export function nomeQueInferiu(t: Tradutor, botName: string): string {
  return botName === 'Berga' ? t('marca.memoriaAuto.oBerga') : botName
}

export function natureLabel(t: Tradutor, nature?: MemoryNature, botName?: string): string {
  switch (nature) {
    case 'said': return t('memoria.auto.natureza.disse')
    case 'inferred': return botName ? t('memoria.auto.natureza.inferiu', { bot: nomeQueInferiu(t, botName) }) : t('memoria.auto.natureza.inferiuSemBot')
    case 'conflict': return t('memoria.auto.natureza.conflito')
    default: return t('memoria.auto.natureza.desconhecida')
  }
}

/** Só apresenta a prova recebida; nunca deduz autoria de writer/texto. */
export function originLabel(t: Tradutor, origin?: MemoryOrigin | null): string {
  // external também é a marca conservadora do legado; não prova uma fonte externa.
  if (origin?.schema !== 1 || origin.kind === 'unknown') return t('memoria.auto.origem.desconhecida')
  if (origin.external || origin.kind === 'external') {
    const details = [...new Set(origin.sources.filter((source) => source.kind === 'external').map((source) => source.detail?.trim()).filter(Boolean))]
    return details.length ? t('memoria.auto.origem.deForaCom', { detalhes: details.join(' · ') }) : t('memoria.auto.origem.deFora')
  }
  if (origin.kind === 'user') return t('memoria.auto.origem.voce')
  if (origin.kind === 'conversation') return t('memoria.auto.origem.conversa')
  return t('memoria.auto.origem.desconhecida')
}

/** O título e o detalhe do recibo, como chaves do catálogo; a tela traduz ao desenhar. */
export function noticeKeys(memory: MemoryWrite): { titulo: ChaveTexto; detalhe?: ChaveTexto } {
  switch (memory.status) {
    case 'done': case 'accepted': return { titulo: 'memoria.auto.aviso.salva' }
    case 'undone': return { titulo: 'memoria.auto.aviso.desfeito' }
    case 'changed': return { titulo: 'memoria.auto.aviso.mudou', detalhe: 'memoria.auto.aviso.mudouDetalhe' }
    case 'declined': return { titulo: 'memoria.auto.aviso.naoSalvo' }
    case 'noChange': return { titulo: 'memoria.auto.aviso.semNova' }
    case 'reserved': return { titulo: 'memoria.auto.aviso.processando', detalhe: 'memoria.auto.aviso.processandoDetalhe' }
  }
  switch (memory.availability) {
    case 'transient': return { titulo: 'memoria.auto.aviso.aindaNao', detalhe: 'memoria.auto.aviso.transitorio' }
    case 'noSelection': return { titulo: 'memoria.auto.aviso.aindaNao', detalhe: 'memoria.auto.aviso.semSelecao' }
    case 'full': return { titulo: 'memoria.auto.aviso.naoSalvo', detalhe: 'memoria.auto.aviso.cheia' }
    case 'expired': return { titulo: 'memoria.auto.aviso.naoSalvo', detalhe: 'memoria.auto.aviso.expirada' }
    case 'ineligible': return { titulo: 'memoria.auto.aviso.naoSalvo', detalhe: 'memoria.auto.aviso.inelegivel' }
    default: return { titulo: memory.status === 'expired' || memory.status === 'notQueued' || memory.status === 'failed' ? 'memoria.auto.aviso.naoSalvo' : 'memoria.auto.aviso.aindaNao', detalhe: 'memoria.auto.aviso.naoConfirmada' }
  }
}

export function noticeText(t: Tradutor, memory: MemoryWrite): { title: string; detail?: string } {
  const keys = noticeKeys(memory)
  const s = t as unknown as (chave: ChaveTexto) => string
  return { title: s(keys.titulo), ...(keys.detalhe ? { detail: s(keys.detalhe) } : {}) }
}

/** O painel não repete recibos de trabalhos concluídos, que podem ter sido desfeitos. */
export function automaticText(t: Tradutor, memory?: MemoryWrite | null) {
  return !memory || memory.status === 'applied' || memory.status === 'noChange' ? null : noticeText(t, memory)
}
