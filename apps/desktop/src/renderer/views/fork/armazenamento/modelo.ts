import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarData, formatarNumero, traduzir, traduzirPlural } from '../../../lib/fork-i18n/index.ts'
// DON-905 (C6): o relatório de armazenamento do Hub (`donnaStorageReport`, só leitura e só do dono), sem DOM nem React.
// `null` é indisponível: nunca vira 0. O total de um bot ou de uma parte só existe quando o Hub mediu tudo dentro dela.
// O banco usado é o arquivo mais o WAL; as partes dos bots são conteúdo medido, não rateio do SQLite, e não somam com ele.
// Nada daqui vai para o mirror: a página relê ao abrir.

export interface Conversas { bytes: number; count: number; oldestAt: number | null }
export interface BotArmazenamento {
  id: string
  name: string
  messages: { main: Conversas | null; extras: Conversas | null; bytes: number | null }
  attachmentsBytes: number | null
  memory: { rowsBytes: number | null; filesBytes: number | null; bytes: number | null }
  whatsapp: { hubBytes: number | null; wacliBytes: number | null; bytes: number | null }
  bytes: number | null
}
export interface RelatorioArmazenamento {
  measuredAt: number
  total: { databaseBytes: number | null; walBytes: number | null; databaseFreeBytes: number | null; diskFreeBytes: number | null }
  /** `null`: o Hub não conseguiu listar os bots; o total continua valendo. */
  bots: BotArmazenamento[] | null
}

const INVALIDA = 'storage_invalid_answer'
const invalida = (): never => { throw new Error(INVALIDA) }
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : invalida())
const inteiro = (v: unknown): number => (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : invalida())
const talvez = (v: unknown): number | null => (v === null ? null : inteiro(v))
const texto = (v: unknown): string => (typeof v === 'string' ? v : invalida())

function conversas(v: unknown): Conversas | null {
  if (v === null) return null
  const o = obj(v)
  return { bytes: inteiro(o.bytes), count: inteiro(o.count), oldestAt: talvez(o.oldestAt) }
}

function bot(v: unknown): BotArmazenamento {
  const o = obj(v), m = obj(o.messages), mem = obj(o.memory), wa = obj(o.whatsapp)
  return {
    id: texto(o.id),
    name: texto(o.name),
    messages: { main: conversas(m.main), extras: conversas(m.extras), bytes: talvez(m.bytes) },
    attachmentsBytes: talvez(o.attachmentsBytes),
    memory: { rowsBytes: talvez(mem.rowsBytes), filesBytes: talvez(mem.filesBytes), bytes: talvez(mem.bytes) },
    whatsapp: { hubBytes: talvez(wa.hubBytes), wacliBytes: talvez(wa.wacliBytes), bytes: talvez(wa.bytes) },
    bytes: talvez(o.bytes),
  }
}

/** Valida a resposta inteira; qualquer campo fora do contrato recusa tudo (`storage_invalid_answer`). */
export function lerRelatorio(raw: unknown): RelatorioArmazenamento {
  const o = obj(raw), t = obj(o.total)
  return {
    measuredAt: inteiro(o.measuredAt),
    total: { databaseBytes: talvez(t.databaseBytes), walBytes: talvez(t.walBytes), databaseFreeBytes: talvez(t.databaseFreeBytes), diskFreeBytes: talvez(t.diskFreeBytes) },
    bots: o.bots === null ? null : Array.isArray(o.bots) ? o.bots.map(bot) : invalida(),
  }
}

const NUM_UNIDADES = 5

/** Tamanho em base 1000, como o Finder: "0 B", "999 B", "1,5 KB", "15 KB", "1,2 GB". `null` é "Indisponível". */
export function tamanho(bytes: number | null, idioma: AppLanguage = 'pt-BR'): string {
  if (bytes === null) return traduzir(idioma, 'armazenamento.indisponivel')
  let valor = bytes, i = 0
  // Arredonda como vai aparecer antes de escolher a unidade: 999,5 KB vira "1 MB", não "1.000 KB".
  const mostrado = (v: number) => (i === 0 || v >= 10 ? Math.round(v) : Math.round(v * 10) / 10)
  while (i < NUM_UNIDADES - 1 && mostrado(valor) >= 1000) { valor /= 1000; i++ }
  return traduzir(idioma, 'armazenamento.tamanho', { valor: formatarNumero(idioma, mostrado(valor)), unidade: traduzir(idioma, 'armazenamento.unidades').split(', ')[i]! })
}

/** O banco usado: arquivo + WAL. Uma das duas faltando, o total é indisponível (as partes aparecem no detalhe). */
export const bancoUsado = (t: RelatorioArmazenamento['total']) => (t.databaseBytes === null || t.walBytes === null ? null : t.databaseBytes + t.walBytes)

/** Maior primeiro; indisponíveis no fim; empate pelo nome. */
export function ordenarBots(bots: readonly BotArmazenamento[], idioma: AppLanguage = 'pt-BR'): BotArmazenamento[] {
  return [...bots].sort((a, b) => (a.bytes === null) !== (b.bytes === null) ? (a.bytes === null ? 1 : -1) : (b.bytes ?? 0) - (a.bytes ?? 0) || a.name.localeCompare(b.name, idioma))
}

/** A fração de cada bot no conteúdo medido dos bots (só os conhecidos); `null` sem medida ou sem nada medido. */
export function fracoes(bots: readonly BotArmazenamento[]): Map<string, number | null> {
  const soma = bots.reduce((s, b) => s + (b.bytes ?? 0), 0)
  return new Map(bots.map((b) => [b.id, b.bytes === null || soma === 0 ? null : b.bytes / soma]))
}

/** "1.234 mensagens · desde 3 de jan. de 2026"; sem mensagens, só a contagem. */
export function detalheConversas(c: Conversas | null, timeZone?: string, idioma: AppLanguage = 'pt-BR'): string | null {
  if (!c) return null
  const mensagens = traduzirPlural(idioma, 'armazenamento.mensagens', c.count)
  if (c.oldestAt === null) return mensagens
  const desde = traduzir(idioma, 'armazenamento.desde', { data: formatarData(idioma, new Date(c.oldestAt), { day: 'numeric', month: 'short', year: 'numeric', timeZone }) })
  return traduzir(idioma, 'armazenamento.conversasDetalhe', { mensagens, desde })
}

export const horaMedida = (ms: number, timeZone?: string, idioma: AppLanguage = 'pt-BR') => formatarData(idioma, new Date(ms), { hour: '2-digit', minute: '2-digit', timeZone })

export const HUB_ANTIGO = traduzir('pt-BR', 'armazenamento.hubAntigo')
export const SO_DONO = traduzir('pt-BR', 'armazenamento.soDono')

/** O texto da falha, pelo status do Hub (`api::error_status`): 404 método ausente, 403/401 sem ser dono. */
export function textoDaFalha(e: unknown, idioma: AppLanguage = 'pt-BR'): string {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
  if (status === 404 || /unknown method/i.test(message)) return traduzir(idioma, 'armazenamento.hubAntigo')
  if (status === 403 || status === 401) return traduzir(idioma, 'armazenamento.soDono')
  if (message.includes(INVALIDA)) return traduzir(idioma, 'armazenamento.respostaInvalida')
  return traduzir(idioma, 'armazenamento.falha')
}
