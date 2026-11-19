// Entrega 4 (MX-7): Ajustes → Atividade. O que o Bot de fundo fez, lido do Hub (`backgroundHistory`, só acesso
// local, até 50 registros). Sem DOM nem React: leitura com cerca por conexão, filtro, dias e totais.
import { readHistory, type HistoryEntry } from '../bot-de-fundo/model.ts'

export interface Transport { call<T>(method: string, body?: unknown): Promise<T> }
export type Erro = 'semConexao' | 'indisponivel' | 'invalido'
export interface Estado { entradas: HistoryEntry[] | null; lendo: boolean; erro: Erro | null }
export type Filtro = 'todas' | 'falhas'

/** Lê o histórico do computador; uma resposta de outra conexão ou leitura antiga é descartada. */
export class Atividade {
  private valor: Estado = { entradas: null, lendo: false, erro: null }
  private ouvintes = new Set<() => void>()
  private leitura = 0
  private readonly link: () => Transport | null
  constructor(link: () => Transport | null) { this.link = link }
  subscribe = (f: () => void) => { this.ouvintes.add(f); return () => { this.ouvintes.delete(f) } }
  getSnapshot = () => this.valor
  private mudar(p: Partial<Estado>) { this.valor = { ...this.valor, ...p }; for (const f of this.ouvintes) f() }
  fechar() { this.leitura++ }
  async ler() {
    const client = this.link()
    const minha = ++this.leitura
    if (!client) { this.mudar({ lendo: false, erro: 'semConexao' }); return }
    // Releitura: as linhas da última leitura ficam na tela até a resposta nova (sem piscar); com erro, ficam também,
    // e a tela avisa. Só a primeira carga, sem dados, mostra "Lendo a atividade…".
    this.mudar({ lendo: true, erro: null })
    try {
      const bruto = await client.call('backgroundHistory', {})
      if (minha !== this.leitura || this.link() !== client) return
      try { this.mudar({ entradas: maisNovasPrimeiro(readHistory(bruto)), lendo: false, erro: null }) } catch { this.mudar({ lendo: false, erro: 'invalido' }) }
    } catch {
      if (minha === this.leitura) this.mudar({ lendo: false, erro: 'indisponivel' })
    }
  }
}

/** O Hub acrescenta no fim (`background_history.rs`, `history.push`): o mais novo é o último. A tela mostra o mais
 * novo primeiro, numa cópia invertida (revisão R1). */
export const maisNovasPrimeiro = <T,>(entradas: readonly T[]): T[] => [...entradas].reverse()

export const filtrar = (entradas: readonly HistoryEntry[], filtro: Filtro) => filtro === 'falhas' ? entradas.filter((e) => e.status === 'failed') : [...entradas]

/** Tokens de um registro: entrada + saída (o cache aparece só no detalhe). Sem uso informado: `null`, não zero. */
export const tokens = (e: HistoryEntry) => e.usage ? e.usage.inputTokens + e.usage.outputTokens : null

/** Agrupa por dia local, na ordem recebida (a leitura já deixa o mais novo primeiro). `semData` junta os sem `at`. */
export function porDia(entradas: readonly HistoryEntry[], agora = Date.now()): { dia: 'hoje' | 'ontem' | number | 'semData'; entradas: HistoryEntry[] }[] {
  const inicio = (ms: number) => { const d = new Date(ms); d.setHours(0, 0, 0, 0); return d.getTime() }
  const hoje = inicio(agora), ontem = inicio(hoje - 1)
  const grupos: { dia: 'hoje' | 'ontem' | number | 'semData'; entradas: HistoryEntry[] }[] = []
  for (const e of entradas) {
    const d = e.at > 0 ? inicio(e.at) : null
    const dia = d === null ? 'semData' : d === hoje ? 'hoje' : d === ontem ? 'ontem' : d
    const ultimo = grupos[grupos.length - 1]
    if (ultimo && ultimo.dia === dia) ultimo.entradas.push(e)
    else grupos.push({ dia, entradas: [e] })
  }
  return grupos
}

export const resumo = (entradas: readonly HistoryEntry[]) => ({ total: entradas.length, falhas: entradas.filter((e) => e.status === 'failed').length })

/** Decisão do dono (11/11): páginas numeradas, 10 por página. Página fora do intervalo vira a mais próxima. */
export const POR_PAGINA = 10
export function paginar<T>(lista: readonly T[], pedida: number, porPagina = POR_PAGINA) {
  const paginas = Math.max(1, Math.ceil(lista.length / porPagina))
  const pagina = Math.min(Math.max(1, Math.trunc(pedida) || 1), paginas)
  const inicio = (pagina - 1) * porPagina
  const itens = lista.slice(inicio, inicio + porPagina)
  return { itens, pagina, paginas, total: lista.length, de: lista.length ? inicio + 1 : 0, ate: inicio + itens.length }
}

/** A página para onde a tecla leva no paginador, ou `null`: ← → uma página (espelhado no RTL), Home e End às pontas. */
export function paginaPorTecla(tecla: string, atual: number, paginas: number, rtl: boolean): number | null {
  const passo = tecla === 'ArrowRight' ? (rtl ? -1 : 1) : tecla === 'ArrowLeft' ? (rtl ? 1 : -1) : 0
  const alvo = passo ? atual + passo : tecla === 'Home' ? 1 : tecla === 'End' ? paginas : null
  return alvo === null || alvo < 1 || alvo > paginas || alvo === atual ? null : alvo
}
