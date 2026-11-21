// ajustes-tela-01: Ajustes é uma tela, como Rotinas, não mais
// uma folha. Este modelo, sem DOM nem React, diz se a tela está aberta, em que página e em que aba. Abrir
// Ajustes fecha Rotinas (um lugar atual por vez): quem liga as duas telas é AjustesTela.tsx.
import { Observable } from '../../../lib/observable.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

/** As páginas de Ajustes (a lista lateral de SettingsView). */
export type SettingsPage = 'general' | 'appearance' | 'sidebar' | 'conversation' | 'memory' | 'never-do' | 'instructions' | 'ai-accounts' | 'delegation' | 'background' | 'activity' | 'workplaces' | 'computers' | 'access' | 'voice' | 'usage' | 'storage' | 'updates' | 'app-icon'

/** D454 (fatia 4): o rótulo é a chave do catálogo; quem desenha traduz. */
export interface Aba { id: string; chave: ChaveTexto }

/** Abas só nas páginas longas (decisão D229 do coordenador); as curtas ficam inteiras. */
export const abasDe: Partial<Record<SettingsPage, readonly Aba[]>> = {
  'ai-accounts': [{ id: 'contas', chave: 'ajustes.aba.contas' }, { id: 'motores', chave: 'ajustes.aba.motores' }],
  // Depois do #166 (D229): as três partes da tela Voz, cada uma com o seu cartão.
  voice: [{ id: 'ouvir', chave: 'ajustes.aba.ouvir' }, { id: 'vivo', chave: 'ajustes.aba.vivo' }, { id: 'chaves', chave: 'ajustes.aba.chaves' }],
  // Bot de fundo (regra do dono 15/11): só o Space ativo; sem abas.
  // MX-145 E2: o painel de uso; os ids são os de `ABAS_USO` (fork/custo/modelo.ts).
  usage: [{ id: 'conta', chave: 'ajustes.aba.uso.conta' }, { id: 'modelo', chave: 'ajustes.aba.uso.modelo' }, { id: 'bot', chave: 'ajustes.aba.uso.bot' }],
}

export class TelaAjustes extends Observable {
  /** A página aberta; `null` com a tela fechada. */
  page: SettingsPage | null = null
  /** A última aba de cada página nesta sessão do app. */
  #abas = new Map<SettingsPage, string>()
  /** O que já havia ao abrir (Rotinas aberta): fecha antes. */
  aoAbrir: () => void = () => {}
  /** Devolve o foco ao fechar pelo Voltar ou pelo Esc (o DOM fica com a tela). */
  aoFechar: () => void = () => {}

  get open() { return this.page !== null }

  /** A aba mostrada: a pedida, senão a última da sessão, senão a primeira; `null` em página sem abas. */
  get aba(): string | null {
    const lista = this.page ? abasDe[this.page] : undefined
    if (!lista?.length || !this.page) return null
    const salva = this.#abas.get(this.page)
    return lista.some((a) => a.id === salva) ? salva! : lista[0]!.id
  }

  show(page: SettingsPage = 'general', aba?: string) {
    this.aoAbrir()
    this.page = page
    // Aba pedida e desconhecida: a primeira, não a última da sessão (uma aba omitida reabre a última).
    if (aba !== undefined && !this.#lembrar(page, aba)) this.#abas.delete(page)
    this.changed()
  }

  /** Troca de página dentro da tela aberta. */
  setPage(page: SettingsPage) {
    if (!this.open || this.page === page) return
    this.page = page
    this.changed()
  }

  setAba(aba: string) {
    if (!this.page || !this.#lembrar(this.page, aba)) return
    this.changed()
  }

  /** `restoreFocus`: Voltar e Esc devolvem o foco; ir a outro lugar (um space, Rotinas, uma notificação) não. */
  close(restoreFocus = true) {
    if (!this.open) return
    this.page = null
    this.changed()
    if (restoreFocus) this.aoFechar()
  }

  #lembrar(page: SettingsPage, aba: string) {
    if (!abasDe[page]?.some((a) => a.id === aba)) return false
    this.#abas.set(page, aba)
    return true
  }
}

export const settingsScreen = new TelaAjustes()

/** O Voltar do cabeçalho e o Esc fazem a mesma coisa: voltar à conversa (ou ao editor, ou ao space). */
export const voltarDosAjustes = (tela: TelaAjustes = settingsScreen) => tela.close()

/**
 * O nome que a lateral dos Ajustes mostra no cabeçalho do primeiro grupo: o do Space ativo no seletor do topo.
 * `null` com o Pessoal aberto (nenhum Space ativo): aí a lateral usa o rótulo "Pessoal". O `record` pode ainda não
 * ter chegado; a lista de Spaces do computador dele cobre esse intervalo.
 */
export function nomeDoSpaceAtivo(ativo: { spaceId: string; record: { name: string } | null } | null, spaces: readonly { id: string; name: string }[] | undefined): string | null {
  if (!ativo) return null
  const nome = (ativo.record?.name ?? spaces?.find((s) => s.id === ativo.spaceId)?.name ?? '').trim()
  return nome || null
}
