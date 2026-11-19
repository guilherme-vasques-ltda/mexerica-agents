// UI generativa no chat, D273/111A: modelo puro do cartão de pergunta (ChoiceCard real, fatia
// preparatória do marco 2). Sem React, rede, IPC, relógio ou armazenamento: o componente renderiza o
// que este modelo projeta e devolve ao consumidor uma intenção tipada. Quem decide estado, prazo e
// resposta é o Hub (host/src/fork/genui/contract.rs e storage.rs); nada aqui prova autoridade.
//
// O parser é defensivo e espelha o validador fechado `choice` v1 do Hub (mesmos limites e códigos de
// erro). Ele não substitui aquele validador, não concede confiança a um payload e não é um fio novo:
// os campos de visualização (ID da pergunta, revisão, estado, resposta, envio) são props privadas do
// componente até existir o contrato de publicação (G1b).

import { parseClosedJson } from './json-fechado.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'

/** Limites do contrato `choice` v1, iguais aos do Hub. Texto conta pontos de código (`chars()`). */
export const CHOICE_LIMITS = {
  maxBytes: 64 * 1024,
  minOptions: 1,
  maxOptions: 8,
  title: 160,
  label: 160,
  summary: 2_000,
  question: 2_000,
  answerText: 4_000,
  optionIdBytes: 160,
} as const

export interface ChoiceOption {
  id: string
  label: string
}

/** O payload `choice` versão 1 (catálogo fechado do Hub; só este componente existe). */
export interface ChoiceV1 {
  version: 1
  component: 'choice'
  title: string
  summary: string
  data: { question: string; options: ChoiceOption[] }
}

/** A resposta tipada, igual a `Answer` do Hub. É texto de conversa: nunca aprovação, ação ou preferência. */
export type ChoiceAnswer = { kind: 'option'; id: string } | { kind: 'none' } | { kind: 'text'; text: string }

/** Códigos estáveis do Hub; nenhum ecoa o conteúdo recusado. */
export type ChoiceError =
  | 'genui_payload_too_large'
  | 'genui_invalid_payload'
  | 'genui_unsupported_version'
  | 'genui_invalid_text'
  | 'genui_invalid_options'
  | 'genui_invalid_option_id'
  | 'genui_duplicate_option_id'
  | 'genui_unknown_option'

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: ChoiceError }

const fail = <T>(error: ChoiceError): Parsed<T> => ({ ok: false, error })

// Espaço em branco Unicode (propriedade White_Space), o mesmo conjunto que `str::trim` do Rust remove.
const WHITE = /^[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]*$/
// Controle Cc fora de nova linha e tab (`char::is_control` do Rust).
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/
// Metade de par substituto solta: o JSON do Hub recusa; aqui também não vira texto.
const LONE_SURROGATE = /\p{Cs}/u
const OPTION_ID = /^[A-Za-z0-9_.-]+$/

// Leitura de dados sem executar código do valor: só objetos simples (protótipo Object ou nulo) e
// arrays com propriedades de dados próprias. Getters, `toJSON` e protótipos estranhos recusam o valor
// antes de qualquer serialização. (Um Proxy ainda pode interceptar a introspecção; o valor vem do
// consumidor, nunca de código do modelo, e o Hub continua sendo o validador que decide.)
// Objeto de outro realm (iframe, vm) tem outro protótipo e é recusado: falha fechada.
function own(v: object, key: string): { ok: true; value: unknown } | { ok: false } {
  const d = Object.getOwnPropertyDescriptor(v, key)
  return d && 'value' in d ? { ok: true, value: d.value } : { ok: false }
}

function plainObject(v: unknown): v is object {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false
  const proto = Object.getPrototypeOf(v)
  return (proto === Object.prototype || proto === null) && Object.getOwnPropertySymbols(v).length === 0
}

/** Objeto fechado: exatamente estas chaves, todas propriedades de dados próprias; nenhuma a mais. */
function closed(v: unknown, keys: readonly string[]): Record<string, unknown> | null {
  if (!plainObject(v)) return null
  const names = Object.getOwnPropertyNames(v)
  if (names.length !== keys.length || !keys.every((k) => names.includes(k))) return null
  const out: Record<string, unknown> = {}
  for (const k of keys) {
    const got = own(v, k)
    if (!got.ok) return null
    out[k] = got.value
  }
  return out
}

/** Array simples: índices 0..n-1 como propriedades de dados, mais `length`, nada além. */
function plainArray(v: unknown): unknown[] | null {
  if (!Array.isArray(v) || Object.getPrototypeOf(v) !== Array.prototype || Object.getOwnPropertySymbols(v).length > 0) return null
  const len = own(v, 'length')
  if (!len.ok || typeof len.value !== 'number') return null
  const n = len.value
  if (n > 64 * 1024 || Object.getOwnPropertyNames(v).length !== n + 1) return null
  const out: unknown[] = []
  for (let i = 0; i < n; i++) {
    const got = own(v, String(i))
    if (!got.ok) return null
    out.push(got.value)
  }
  return out
}

/** Texto aceito pelo Hub: não vazio após aparar, até `max` pontos de código, sem controle além de \n e \t. */
export function validText(value: string, max: number): boolean {
  return !WHITE.test(value) && [...value].length <= max && !CONTROL.test(value)
}

const utf8Bytes = (s: string) => new TextEncoder().encode(s).length

/**
 * Lê um `choice` v1 vindo de fora (texto JSON ou valor já decodificado). Catálogo fechado e versionado:
 * qualquer chave extra, tipo errado, componente ou versão diferentes recusam o payload inteiro, sem
 * truncar nem consertar. O resultado é uma cópia nova com só os campos do contrato.
 */
export function parseChoice(input: unknown): Parsed<ChoiceV1> {
  let value: unknown = input
  if (typeof input === 'string') {
    // O limite de 64 KiB do Hub vale para o JSON bruto, antes de decodificar.
    if (utf8Bytes(input) > CHOICE_LIMITS.maxBytes) return fail('genui_payload_too_large')
    const parsed = parseClosedJson(input, { unsignedIntegerFields: ['version'] })
    if (!parsed.ok) return fail(parsed.error)
    value = parsed.value
  }
  const top = closed(value, ['version', 'component', 'title', 'summary', 'data'])
  if (!top) return fail('genui_invalid_payload')
  const { version, component, title, summary } = top
  if (typeof version !== 'number' || !Number.isInteger(version) || version < 0 || version > 255) return fail('genui_invalid_payload')
  if (component !== 'choice') return fail('genui_invalid_payload')
  if (typeof title !== 'string' || typeof summary !== 'string') return fail('genui_invalid_payload')
  const data = closed(top.data, ['question', 'options'])
  if (!data) return fail('genui_invalid_payload')
  const question = data.question
  const options = plainArray(data.options)
  if (typeof question !== 'string' || !options) return fail('genui_invalid_payload')
  const copied: ChoiceOption[] = []
  for (const item of options) {
    const o = closed(item, ['id', 'label'])
    if (!o || typeof o.id !== 'string' || typeof o.label !== 'string') return fail('genui_invalid_payload')
    copied.push({ id: o.id, label: o.label })
  }
  if ([title, summary, question, ...copied.flatMap((o) => [o.id, o.label])].some((s) => LONE_SURROGATE.test(s))) return fail('genui_invalid_payload')
  const canonical: ChoiceV1 = { version: 1, component: 'choice', title, summary, data: { question, options: copied } }
  // Valor já decodificado: confere a forma serializada da cópia canônica (só dados, nada do original).
  if (typeof input !== 'string' && utf8Bytes(JSON.stringify({ ...canonical, version })) > CHOICE_LIMITS.maxBytes) return fail('genui_payload_too_large')
  if (version !== 1) return fail('genui_unsupported_version')
  if (!validText(title, CHOICE_LIMITS.title) || !validText(summary, CHOICE_LIMITS.summary) || !validText(question, CHOICE_LIMITS.question)) return fail('genui_invalid_text')
  if (copied.length < CHOICE_LIMITS.minOptions || copied.length > CHOICE_LIMITS.maxOptions) return fail('genui_invalid_options')
  const ids = new Set<string>()
  for (const o of copied) {
    // ID opaco e ASCII; nunca é destino de navegação, comando ou ação.
    if (o.id.length > CHOICE_LIMITS.optionIdBytes || !OPTION_ID.test(o.id)) return fail('genui_invalid_option_id')
    if (ids.has(o.id)) return fail('genui_duplicate_option_id')
    ids.add(o.id)
    if (!validText(o.label, CHOICE_LIMITS.label)) return fail('genui_invalid_text')
  }
  return { ok: true, value: canonical }
}

/** Confere uma resposta contra a pergunta (forma fechada do Hub; opção precisa existir; texto nos limites). */
export function parseAnswer(input: unknown, choice: ChoiceV1): Parsed<ChoiceAnswer> {
  if (!plainObject(input)) return fail('genui_invalid_payload')
  const kind = own(input, 'kind')
  if (!kind.ok) return fail('genui_invalid_payload')
  // Fechado como no Hub: `none` com qualquer campo a mais é recusado.
  if (kind.value === 'none') return closed(input, ['kind']) ? { ok: true, value: { kind: 'none' } } : fail('genui_invalid_payload')
  if (kind.value === 'option') {
    const a = closed(input, ['kind', 'id'])
    if (!a || typeof a.id !== 'string') return fail('genui_invalid_payload')
    const id = a.id
    return choice.data.options.some((o) => o.id === id) ? { ok: true, value: { kind: 'option', id } } : fail('genui_unknown_option')
  }
  if (kind.value === 'text') {
    const a = closed(input, ['kind', 'text'])
    if (!a || typeof a.text !== 'string') return fail('genui_invalid_payload')
    const text = a.text
    return validText(text, CHOICE_LIMITS.answerText) && !LONE_SURROGATE.test(text) ? { ok: true, value: { kind: 'text', text } } : fail('genui_invalid_text')
  }
  return fail('genui_invalid_payload')
}

// ---------------------------------------------------------------------------------------------------
// Estado projetado por props. O Hub é a única fonte de pendente/respondida/cancelada/expirada; o
// consumidor só informa o próprio envio transitório. Nada aqui espera, reenvia, agenda ou guarda.

/** Estado da pergunta como o Hub o registrou (horários em ms desde a época, do relógio do Hub). */
export type ChoiceHubState =
  | { state: 'pending'; revision: number }
  | { state: 'answered'; revision: number; answer: unknown; answeredAt: number }
  | { state: 'cancelled'; revision: number; at?: number | null }
  | { state: 'expired'; revision: number; at?: number | null }

/** Envio transitório para o dono exato visto na intenção. Prop privada, nunca prova Hub ou fio. */
export interface ChoiceSubmission extends ChoiceIntent {
  status: 'sending' | 'unconfirmed' | 'error'
}

/** Callback privado com a mesma identidade canônica do rascunho; não é DTO público ou autoridade. */
export interface ChoiceIntent extends DraftOwner {
  answer: ChoiceAnswer
}

/** O que a pessoa está montando, só no cartão: não é resposta até Enviar resposta. */
export type ChoiceDraft = { pick: { kind: 'option'; id: string } | { kind: 'none' } | { kind: 'text' } | null; text: string }

export const emptyDraft: ChoiceDraft = { pick: null, text: '' }

/** Escrever escolhe a resposta escrita; apagar tudo volta para nada escolhido. */
export function typeText(draft: ChoiceDraft, text: string): ChoiceDraft {
  return { pick: text.length > 0 ? { kind: 'text' } : draft.pick?.kind === 'text' ? null : draft.pick, text }
}

/** Marcar uma opção (ou Nenhuma destas) troca a resposta; o texto fica no campo, mas não sai. */
export function pickOption(draft: ChoiceDraft, pick: { kind: 'option'; id: string } | { kind: 'none' }): ChoiceDraft {
  return { pick, text: draft.text }
}

export type DraftCheck =
  | { ok: true; answer: ChoiceAnswer }
  | { ok: false; reason: 'empty' | 'text_blank' | 'text_too_long' | 'text_invalid' | 'unknown_option' }

/** A resposta que Enviar resposta mandaria agora, ou por que não manda. */
export function draftAnswer(draft: ChoiceDraft, choice: ChoiceV1): DraftCheck {
  const pick = draft.pick
  if (!pick) return { ok: false, reason: 'empty' }
  if (pick.kind === 'none') return { ok: true, answer: { kind: 'none' } }
  if (pick.kind === 'option') {
    return choice.data.options.some((o) => o.id === pick.id) ? { ok: true, answer: { kind: 'option', id: pick.id } } : { ok: false, reason: 'unknown_option' }
  }
  const text = draft.text.replace(/^\s+|\s+$/g, '')
  if (text.length === 0) return { ok: false, reason: 'text_blank' }
  if ([...text].length > CHOICE_LIMITS.answerText) return { ok: false, reason: 'text_too_long' }
  if (!validText(text, CHOICE_LIMITS.answerText) || LONE_SURROGATE.test(text)) return { ok: false, reason: 'text_invalid' }
  return { ok: true, answer: { kind: 'text', text } }
}

/** Rascunho a partir de uma resposta (ao voltar de um erro de envio, a escolha continua no cartão). */
export function draftFrom(answer: ChoiceAnswer): ChoiceDraft {
  return answer.kind === 'text' ? { pick: { kind: 'text' }, text: answer.text } : { pick: answer, text: '' }
}

/** Identidade privada compartilhada por rascunho, intenção e envio: pergunta, revisão e payload canônico. */
export interface DraftOwner {
  questionId: string
  revision: number
  snapshot: string
}

export interface HeldDraft {
  owner: DraftOwner
  /** null: ainda não há rascunho. emptyDraft: a pessoa pode tê-lo esvaziado deliberadamente. */
  draft: ChoiceDraft | null
}

/** Snapshot canônico (ordem fixa de chaves) de um payload já validado. */
export const choiceSnapshot = (choice: ChoiceV1) => JSON.stringify(choice)

export const sameDraftOwner = (a: DraftOwner, b: DraftOwner) =>
  a.questionId === b.questionId && a.revision === b.revision && a.snapshot === b.snapshot

/**
 * Projeta só o rascunho do dono atual. O componente elimina o estado antigo ao trocar de dono e
 * materializa a restauração de erro uma vez; um rascunho vazio explícito continua vazio.
 */
export function currentDraft(held: HeldDraft | null, owner: DraftOwner, view: ChoiceView): ChoiceDraft {
  if (held && sameDraftOwner(held.owner, owner) && held.draft !== null) return held.draft
  return view.status === 'error' && view.shown ? draftFrom(view.shown) : emptyDraft
}

export type ChoiceStatus = 'pending' | 'sending' | 'unconfirmed' | 'error' | 'answered' | 'cancelled' | 'expired' | 'unavailable'

/** A vista do cartão: o que mostrar, se os controles valem e a resposta em destaque. */
export interface ChoiceView {
  status: ChoiceStatus
  /** Rádios, campo e Enviar ativos (só pendente sem envio em curso, ou depois de um erro). */
  editable: boolean
  /** A resposta que o cartão mostra como enviada/enviando (nunca a seleção local). */
  shown: ChoiceAnswer | null
  /** A revisão que uma intenção nova carrega. */
  revision: number
  /** Horário do Hub para o estado final, quando ele mandou. */
  at: number | null
}

/**
 * Projeta estado Hub e envio do consumidor. Envio sem vínculo completo ou de outro dono é ignorado
 * antes de projetar/restaurar; final do Hub vence. Respondida só com resposta válida vinda do Hub.
 */
export function projectChoice(choice: ChoiceV1, hub: ChoiceHubState, submission: ChoiceSubmission | null, owner: DraftOwner): ChoiceView {
  const base = { revision: hub.revision, at: null as number | null }
  if (hub.state === 'answered') {
    const answer = parseAnswer(hub.answer, choice)
    if (!answer.ok || !Number.isFinite(hub.answeredAt)) return { ...base, status: 'unavailable', editable: false, shown: null }
    return { ...base, status: 'answered', editable: false, shown: answer.value, at: hub.answeredAt }
  }
  if (hub.state === 'cancelled' || hub.state === 'expired') {
    return { ...base, status: hub.state, editable: false, shown: null, at: Number.isFinite(hub.at) ? (hub.at as number) : null }
  }
  if (!submission || !sameDraftOwner(submission, owner) || submission.revision !== hub.revision) return { ...base, status: 'pending', editable: true, shown: null }
  const answer = parseAnswer(submission.answer, choice)
  if (!answer.ok) return { ...base, status: 'pending', editable: true, shown: null }
  // O tipo TS não valida props em execução. Só estes estados transitórios podem chegar à vista;
  // um enum terminal (ou qualquer outro valor) no envio nunca registra estado do Hub.
  switch (submission.status) {
    case 'sending': return { ...base, status: 'sending', editable: false, shown: answer.value }
    case 'unconfirmed': return { ...base, status: 'unconfirmed', editable: false, shown: answer.value }
    case 'error': return { ...base, status: 'error', editable: true, shown: answer.value }
    default: return { ...base, status: 'pending', editable: true, shown: null }
  }
}

// ---------------------------------------------------------------------------------------------------
// Textos em conversa.escolha.* (catálogo, seis idiomas). Sempre texto: nada vira HTML, link ou estilo.

const LETTERS = 'ABCDEFGH'

/** A letra de cada opção, na ordem do payload (até oito). */
export const optionKey = (index: number) => LETTERS[index] ?? String(index + 1)

/** A resposta como uma linha legível: "B — rótulo", "Nenhuma destas" ou "sua resposta escrita". */
export function answerLine(t: Tradutor, answer: ChoiceAnswer, choice: ChoiceV1): string {
  if (answer.kind === 'none') return t('conversa.escolha.nenhuma')
  if (answer.kind === 'text') return t('conversa.escolha.respostaEscrita')
  const index = choice.data.options.findIndex((o) => o.id === answer.id)
  return index < 0 ? t('conversa.escolha.opcaoIndisponivel') : t('conversa.escolha.opcaoLinha', { letra: optionKey(index), rotulo: choice.data.options[index]!.label })
}

export interface ClockText {
  /** "10:42" ou "18:00 de ontem", no fuso do app. */
  time: (ms: number) => string
}

/** O texto da pílula de estado (vai junto do ícone; nunca só cor). */
export function statusLabel(t: Tradutor, view: ChoiceView, clock: ClockText): string {
  switch (view.status) {
    case 'pending': return t('conversa.escolha.estado.pendente')
    case 'sending': return t('conversa.escolha.estado.enviando')
    case 'unconfirmed': return t('conversa.escolha.estado.semConfirmacao')
    case 'error': return t('conversa.escolha.estado.naoEnviada')
    case 'answered': return view.at === null ? t('conversa.escolha.estado.respondida') : t('conversa.escolha.estado.respondidaAs', { hora: clock.time(view.at) })
    case 'expired': return view.at === null ? t('conversa.escolha.estado.expirada') : t('conversa.escolha.estado.expiradaAs', { hora: clock.time(view.at) })
    case 'cancelled': return t('conversa.escolha.estado.encerrada')
    case 'unavailable': return t('conversa.escolha.estado.indisponivel')
  }
}

/** A linha de rodapé do estado (texto explicativo, sem prometer o que o Hub não confirmou). */
export function statusNote(t: Tradutor, view: ChoiceView, clock: ClockText, botName: string): string | null {
  switch (view.status) {
    case 'pending': return null
    case 'sending': return t('conversa.escolha.nota.enviando')
    case 'unconfirmed': return t('conversa.escolha.nota.semConfirmacao')
    case 'error': return t('conversa.escolha.nota.naoEnviada')
    case 'answered': {
      const who = botName.charAt(0).toLocaleUpperCase('pt-BR') + botName.slice(1)
      // Nenhuma destas: a frase diz também que nada é cancelado. Uma frase inteira por caso, sem montar pedaços.
      const nada = view.shown?.kind === 'none'
      if (view.at === null) return t(nada ? 'conversa.escolha.nota.registradaNada' : 'conversa.escolha.nota.registrada', { bot: who })
      return t(nada ? 'conversa.escolha.nota.registradaAsNada' : 'conversa.escolha.nota.registradaAs', { bot: who, hora: clock.time(view.at) })
    }
    case 'expired': return view.at === null ? t('conversa.escolha.nota.expirada') : t('conversa.escolha.nota.expiradaAte', { hora: clock.time(view.at) })
    case 'cancelled': return t('conversa.escolha.nota.encerrada')
    case 'unavailable': return t('conversa.escolha.nota.indisponivel')
  }
}

/** O anúncio da região educada quando o estado muda (só a mudança, nunca o histórico). */
export function statusAnnouncement(t: Tradutor, view: ChoiceView, clock: ClockText): string {
  switch (view.status) {
    case 'pending': return ''
    case 'sending': return t('conversa.escolha.anuncio.enviando')
    case 'unconfirmed': return t('conversa.escolha.anuncio.semConfirmacao')
    case 'error': return t('conversa.escolha.anuncio.naoEnviada')
    case 'answered': return view.at === null ? t('conversa.escolha.anuncio.registrada') : t('conversa.escolha.anuncio.registradaAs', { hora: clock.time(view.at) })
    case 'expired': return t('conversa.escolha.anuncio.expirada')
    case 'cancelled': return t('conversa.escolha.anuncio.encerrada')
    case 'unavailable': return t('conversa.escolha.anuncio.indisponivel')
  }
}

/** A nota ao lado de Enviar resposta, enquanto se monta a resposta. */
export function draftNote(t: Tradutor, check: DraftCheck, choice: ChoiceV1, idioma: AppLanguage): string {
  if (check.ok) {
    return check.answer.kind === 'text' ? t('conversa.escolha.rascunho.escrita') : t('conversa.escolha.rascunho.vai', { resposta: answerLine(t, check.answer, choice) })
  }
  switch (check.reason) {
    case 'empty':
    case 'unknown_option': return t('conversa.escolha.rascunho.vazio')
    case 'text_blank': return t('conversa.escolha.rascunho.branco')
    case 'text_too_long': return t('conversa.escolha.rascunho.longo', { limite: formatarNumero(idioma, CHOICE_LIMITS.answerText) })
    case 'text_invalid': return t('conversa.escolha.rascunho.invalido')
  }
}
