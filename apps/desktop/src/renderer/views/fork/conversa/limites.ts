// DON-907 C4: o contrato do Hub para os limites de uso (docs/reference/design/don-907-c4/README.md). Sem textos nem DOM: a view
// traduz os códigos com `useT` (chaves `conversa.limites.*`). O Hub decide e grava tudo; o app só mostra o estado
// confirmado e manda a intenção.
import type { Entry } from '@shared/models'
import type { AppLanguage } from '@shared/fork-idioma'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'
import { paradaPeloLimite } from '../../../lib/limite-volta.ts'

export interface LimitesDeUso { autoResume: boolean; snooze: boolean }

/** O código estável de `lerLimites` quando a resposta do Hub não tem a forma esperada. */
export const FORMA_INVALIDA = 'limites-forma-invalida'

/** O que o Hub devolve em `donnaUsageLimits`/`donnaUsageLimitsSave`; qualquer outra forma é recusada. */
export function lerLimites(v: unknown): LimitesDeUso {
  const o = v as Partial<LimitesDeUso> | null
  if (!o || typeof o.autoResume !== 'boolean' || typeof o.snooze !== 'boolean') throw new Error(FORMA_INVALIDA)
  return { autoResume: o.autoResume, snooze: o.snooze }
}

/** O status do Hub (`api::error_status`): 404 ou método desconhecido é Hub antigo. */
export function hubAntigo(e: unknown): boolean {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
  return status === 404 || /unknown method/i.test(message)
}

/**
 * A espera exata deste aviso: o aviso, a espera (`waitId`) e o destino. Um clique antigo nunca vale para outro
 * pedido: o Hub confere os cinco campos. A principal vai como `sessionId: null` (o id dela é o da conversa).
 */
export function alvoDaEspera(entry: Entry) {
  const waitId = entry.data.subscriptionWait?.waitId
  if (!waitId) return null
  const session = entry.sessionId && entry.sessionId !== entry.botId ? entry.sessionId : null
  return { botId: entry.data.author ?? entry.botId, chatId: entry.botId, sessionId: session, entryId: entry.id, waitId }
}

export type Acao = 'naoRetomar' | 'retomarAgora'

/**
 * Os botões que o estado confirmado pelo Hub permite, só com `waitId` (Hub antigo: o aviso de sempre, com Tentar
 * de novo e Parar). `retry` = o Hub aceita retomar à mão agora (pausada, retomada automática desligada, limite de
 * volta, ou cancelada antes do envio). Cancelada: só Retomar agora, e só com `retry`; se o pedido já tinha
 * recomeçado, o Hub manda `retry: false` e nada é oferecido. Nunca usa o Parar geral do bot.
 */
export function acoesDaEspera(entry: Entry): Acao[] {
  const w = entry.data.subscriptionWait
  if (!w?.waitId) return []
  const retomar: Acao[] = w.retry ? ['retomarAgora'] : []
  return w.cancelled ? retomar : [...retomar, 'naoRetomar']
}

/**
 * MX-245: "Parou pelo limite do plano; volta às HH:MM" no lugar do texto pronto do Hub, só enquanto a espera é a
 * automática de sempre (ver `paradaPeloLimite`); `null` deixa o texto do Hub, que diz o estado exato. Com o instante
 * de retomada que o Hub já manda (`resumeAt`); sem ele, diz que o provedor não informou quando volta.
 */
export function paradaDaEspera(t: Tradutor, idioma: AppLanguage, entry: Entry, agora: number = Date.now(), fuso?: string): string | null {
  const espera = entry.data.subscriptionWait
  // MX-263: a espera de rotina já leva o nome dela no texto do Hub; a frase
  // genérica o apagaria, então o texto do Hub fica.
  if (entry.data.routineWait) return null
  return espera ? paradaPeloLimite(t, idioma, espera, entry.data.style === 'error', agora, fuso) : null
}
