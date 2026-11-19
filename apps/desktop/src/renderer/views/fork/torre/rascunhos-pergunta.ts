// MB-1b R3 (B1): o rascunho da resposta a uma pergunta do bot vive fora do componente, por Hub e space, chaveado
// pelo entryId da pergunta. Sobrevive à desmontagem (Board sem conexão, stream pausado, reconexão) e volta com o
// mesmo texto, origem e nonce: o estado do envio (enviando/falhou) continua vindo da entrada local do MESMO send.
// Só em memória: nada vai para disco, mirror ou localStorage. Sai quando a pergunta deixa a lista do Hub ou ao descartar.

import type { OriginDraft } from '../../../../shared/guard-hub.ts'
import { answerView, type Responder } from './responder.ts'
import { questionsOf, type TowerSnapshot } from '../../../../shared/fork-torre.ts'

export interface QuestionDraft { botId: string | null; text: string; origin: OriginDraft | undefined; nonce: string | null }

const EMPTY: QuestionDraft = Object.freeze({ botId: null, text: '', origin: undefined, nonce: null })

export class QuestionDrafts {
  private drafts = new Map<string, QuestionDraft>()
  private listeners = new Set<() => void>()

  /** Mesmo objeto enquanto não muda (serve de snapshot do useSyncExternalStore). */
  get(entryId: string): QuestionDraft { return this.drafts.get(entryId) ?? EMPTY }

  set(entryId: string, patch: Partial<QuestionDraft>) {
    this.drafts.set(entryId, { ...this.get(entryId), ...patch })
    this.emit()
  }

  discard(entryId: string) {
    if (this.drafts.delete(entryId)) this.emit()
  }

  /**
   * Fotografia ao vivo: fica só o rascunho de pergunta ainda listada, ou de bot cujas perguntas o Hub não informou
   * agora (indisponível não é "respondida").
   */
  keep(listed: ReadonlySet<string>, unknownBots: ReadonlySet<string>) {
    let changed = false
    for (const [id, d] of this.drafts) {
      if (listed.has(id) || (d.botId !== null && unknownBots.has(d.botId))) continue
      this.drafts.delete(id)
      changed = true
    }
    if (changed) this.emit()
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  private emit() { for (const l of this.listeners) l() }
}

const byPlace = new Map<string, QuestionDrafts>()

/** Um store por Hub e space, enquanto o app está aberto. Outro Hub não vê os rascunhos deste. */
export function questionDrafts(computerId: string, spaceId: string): QuestionDrafts {
  const key = `${computerId}\u0000${spaceId}`
  let drafts = byPlace.get(key)
  if (!drafts) byPlace.set(key, (drafts = new QuestionDrafts()))
  return drafts
}

/** Enviar (ou Tentar de novo): o rascunho guardado, o mesmo nonce se o envio anterior falhou. */
export function submitDraft(responder: Responder, drafts: QuestionDrafts, entryId: string, chatId: string): void {
  const draft = drafts.get(entryId)
  const state = draft.nonce ? responder.sendState(chatId, draft.nonce) : null
  if (!answerView(state).editable) return
  const nonce = responder.answer(chatId, draft.text, draft.origin, draft.nonce ?? undefined)
  if (nonce) drafts.set(entryId, { nonce })
}

/** O que o Board faz a cada fotografia ao vivo: limpa o rascunho de pergunta que o Hub já tirou da lista. */
export function keepDraftsFor(drafts: QuestionDrafts, snapshot: TowerSnapshot) {
  const asked = new Set(snapshot.bots.flatMap((b) => (questionsOf(b) ?? []).map((q) => q.entryId)))
  const unknown = new Set(snapshot.bots.filter((b) => questionsOf(b) === null).map((b) => b.botId))
  drafts.keep(asked, unknown)
}
