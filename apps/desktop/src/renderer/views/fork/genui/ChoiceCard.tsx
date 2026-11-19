import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { radioArrowTarget, radioTabStop } from '../../../lib/radio-group'
import {
  choiceSnapshot,
  currentDraft,
  draftAnswer,
  draftNote,
  optionKey,
  parseChoice,
  pickOption,
  projectChoice,
  sameDraftOwner,
  statusAnnouncement,
  statusLabel,
  statusNote,
  typeText,
  type ChoiceAnswer,
  type ChoiceDraft,
  type ChoiceHubState,
  type ChoiceIntent,
  type ChoiceStatus,
  type ChoiceSubmission,
  type ChoiceV1,
  type ChoiceView,
  type DraftOwner,
  type HeldDraft,
  type ClockText,
} from './escolha-model'
import { formatarData } from '../../../lib/fork-i18n'
import { superficieIdioma, useIdioma, useT, tradutor, type Tradutor } from '../../../lib/fork-i18n/estado'
import type { AppLanguage } from '@shared/fork-idioma'
import './choice-card.css'

// UI generativa no chat, D273/111A: o cartão de pergunta REAL, fatia preparatória do marco 2.
// Proposta aprovada: (arquivo de design privado do projeto de origem, fora deste repositório)
//
// Montado na lista de mensagens por GenuiChoiceRow (G1b), a partir de `entry.data.genuiChoice`. O cartão
// não fala com Hub, IPC ou rede: lê um payload `choice` v1 (validado aqui de novo, defensivamente), o
// estado que o Hub registrou e o envio transitório do consumidor, e entrega ao consumidor só uma
// intenção tipada. Seleção local não é resposta; só o estado do Hub marca Respondida. Escolher é
// conversa: nunca aprovação, permissão ou ação. Todo texto do payload entra como nó de texto.

export interface ChoiceCardProps {
  /** ID durável da pergunta no Hub (visualização; volta na intenção). */
  questionId: string
  /** O payload `choice` v1 como chegou; é validado de novo antes de qualquer render. */
  payload: unknown
  /** Estado registrado pelo Hub. Única fonte de pendente/respondida/encerrada/expirada. */
  hub: ChoiceHubState
  /** Envio transitório vinculado à pergunta/revisão/snapshot da intenção. Outro dono/incompleto é ignorado. */
  submission?: ChoiceSubmission | null
  /** Recebe a intenção ao tocar em Enviar resposta. Sem Promise, sem espera, sem reenvio sozinho. */
  onAnswer: (intent: ChoiceIntent) => void
  /** Como as frases chamam quem perguntou ("a Donna"): "Sua resposta volta para a Donna como texto". */
  botName: string
  /** Nome e hora no topo do balão, quando o cartão abre o balão do bot. */
  author?: { name: string; time: string } | null
  /** Formato dos horários do Hub; o padrão é HH:MM no fuso do app. */
  clock?: ClockText
  /** G1b: o motivo do envio que não valeu, já em texto do app (nunca do Hub); só aparece no estado de erro. */
  errorText?: string | null
}

const relogioPadrao = (t: Tradutor, idioma: AppLanguage): ClockText => ({
  time: (ms) => {
    const at = new Date(ms)
    const hm = formatarData(idioma, at, { hour: '2-digit', minute: '2-digit' })
    const now = new Date()
    const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
    const diff = Math.round((day(now) - day(at)) / 86_400_000)
    if (diff === 0) return hm
    if (diff === 1) return t('conversa.escolha.horaOntem', { hora: hm })
    return t('conversa.escolha.dataHora', { data: formatarData(idioma, at, { day: '2-digit', month: '2-digit' }), hora: hm })
  },
})

export function ChoiceCard(props: ChoiceCardProps) {
  const parsed = useMemo(() => parseChoice(props.payload), [props.payload])
  if (!parsed.ok) return <InvalidChoice author={props.author ?? null} />
  return <ChoiceCardView {...props} choice={parsed.value} />
}

/** Payload fora do contrato: nada dele aparece (nem o motivo, que poderia ecoar o conteúdo). */
function InvalidChoice({ author }: { author: { name: string; time: string } | null }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  return (
    <section className="genui-choice idioma-superficie" {...superficieIdioma(idioma)} role="group" aria-label={t('conversa.escolha.indisponivel')} data-status="invalid">
      <div className="genui-choice-head">
        {author ? <Meta author={author} /> : null}
        <span className="genui-choice-state">
          <Icon name="exclamationmark.circle" size={12} className="genui-choice-state-mark" />
          {t('conversa.escolha.indisponivel')}
        </span>
      </div>
      <p className="genui-choice-line">{t('conversa.escolha.indisponivelDetalhe')}</p>
    </section>
  )
}

function Meta({ author }: { author: { name: string; time: string } }) {
  return (
    <>
      <span className="bubble-meta-name">{author.name}</span>
      <span className="bubble-meta-time">{author.time}</span>
    </>
  )
}

type Pick = NonNullable<ChoiceDraft['pick']>

function ChoiceCardView({ questionId, choice, hub, submission = null, onAnswer, botName, author = null, clock: relogio, errorText = null }: ChoiceCardProps & { choice: ChoiceV1 }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const clock = relogio ?? relogioPadrao(t, idioma)
  const ids = useId()
  const questionEl = `${ids}-q`
  const helpEl = `${ids}-help`
  const textEl = `${ids}-text`
  const noteEl = `${ids}-note`
  const snapshot = useMemo(() => choiceSnapshot(choice), [choice])
  const owner: DraftOwner = { questionId, revision: hub.revision, snapshot }
  const view = projectChoice(choice, hub, submission, owner)

  // Rascunho local: só existe neste cartão e só sai por Enviar resposta. Pertence a uma pergunta,
  // revisão e payload: se qualquer um muda, o rascunho antigo é descartado já neste render, antes de
  // poder virar intenção. Depois de um erro de envio a mesma escolha volta para conferir.
  const [held, setHeld] = useState<HeldDraft>(() => ({ owner, draft: null }))
  const draft = currentDraft(held, owner, view)
  // Ajuste do próprio estado durante o render: React refaz este componente antes de publicar filhos
  // ou handlers. A troca elimina o dono antigo de verdade (A → B → A não recupera A). Restauração
  // de erro consome somente a ausência de rascunho; um campo esvaziado não é ausência.
  if (!sameDraftOwner(held.owner, owner) || (held.draft === null && view.status === 'error' && view.shown)) {
    setHeld({ owner, draft: view.status === 'error' ? draft : null })
  }
  const setDraft = (next: ChoiceDraft) => setHeld({ owner, draft: next })
  const prevStatus = useRef<ChoiceStatus>(view.status)

  // Região educada: anuncia só a mudança de estado, nunca o cartão inteiro nem o histórico.
  const [announcement, setAnnouncement] = useState('')
  useEffect(() => {
    if (prevStatus.current !== view.status) setAnnouncement(statusAnnouncement(t, view, clock))
    prevStatus.current = view.status
  })

  // Foco preservado: se o controle com foco some (Enviar → Enviando…), o foco vai para a linha de
  // estado do próprio cartão, nunca para o começo da página. Não rouba foco que já estava fora.
  const lastFocused = useRef<HTMLElement | null>(null)
  const lineRef = useRef<HTMLParagraphElement | null>(null)
  const firstRadio = useRef<HTMLButtonElement | null>(null)
  useLayoutEffect(() => {
    const gone = lastFocused.current
    const active = document.activeElement
    if (!gone || gone.isConnected || (active && active !== document.body)) return
    lastFocused.current = null
    ;(lineRef.current ?? firstRadio.current)?.focus()
  })

  const check = draftAnswer(draft, choice)
  const send = (answer: ChoiceAnswer) => onAnswer({ ...owner, answer })
  const note = view.status === 'error' && errorText ? errorText : statusNote(t, view, clock, botName)

  return (
    <section
      className="genui-choice idioma-superficie"
      {...superficieIdioma(idioma)}
      role="group"
      aria-labelledby={questionEl}
      aria-describedby={helpEl}
      data-status={view.status}
      onFocus={(e) => { lastFocused.current = e.target as HTMLElement }}
      onBlur={(e) => {
        // Foco que saiu de verdade (clique fora, Tab para outra área) não é devolvido depois. Só a
        // remoção do controle focado conta: aí o elemento já não está no documento.
        const left = e.target as HTMLElement
        queueMicrotask(() => { if (left.isConnected && lastFocused.current === left) lastFocused.current = null })
      }}
    >
      <div className="genui-choice-head">
        {author ? <Meta author={author} /> : null}
        <StatePill view={view} label={statusLabel(t, view, clock)} />
      </div>
      <p className="genui-choice-title" dir="auto">{choice.title}</p>
      <h3 className="genui-choice-question" id={questionEl} dir="auto">{choice.data.question}</h3>
      <p className="genui-choice-help" id={helpEl}>
        <span>{choice.summary}</span> <span>{t('conversa.escolha.volta', { bot: botName })}</span>
      </p>
      {view.editable ? (
        <Editor
          key="editor"
          choice={choice}
          draft={draft}
          setDraft={setDraft}
          questionEl={questionEl}
          textEl={textEl}
          noteEl={noteEl}
          firstRadio={firstRadio}
          canSend={check.ok}
          note={draftNote(t, check, choice, idioma)}
          onSend={() => { if (check.ok) send(check.answer) }}
        />
      ) : (
        <ShownList key="list" choice={choice} view={view} />
      )}
      {note ? (
        <p key="line" ref={lineRef} className={`genui-choice-line is-${view.status}`} tabIndex={-1}>
          <LineMark status={view.status} />
          <span>{note}</span>
        </p>
      ) : null}
      <span className="genui-choice-live" aria-live="polite" aria-atomic="true">{announcement}</span>
    </section>
  )
}

function StatePill({ view, label }: { view: ChoiceView; label: string }) {
  return (
    <span className={`genui-choice-state is-${view.status}`}>
      {view.status === 'sending' ? <Spinner size={12} /> : <Icon name={stateIcon[view.status]} size={12} weight="semibold" className="genui-choice-state-mark" />}
      {label}
    </span>
  )
}

const stateIcon: Record<ChoiceStatus, string> = {
  pending: 'text.bubble',
  sending: 'text.bubble',
  unconfirmed: 'exclamationmark.circle',
  error: 'exclamationmark.triangle',
  answered: 'checkmark',
  expired: 'clock',
  cancelled: 'xmark.circle',
  unavailable: 'exclamationmark.circle',
}

function LineMark({ status }: { status: ChoiceStatus }) {
  if (status === 'sending') return <Spinner size={12} />
  return <Icon name={stateIcon[status]} size={12} className="genui-choice-line-mark" />
}

function Editor({ choice, draft, setDraft, questionEl, textEl, noteEl, firstRadio, canSend, note, onSend }: {
  choice: ChoiceV1
  draft: ChoiceDraft
  setDraft: (d: ChoiceDraft) => void
  questionEl: string
  textEl: string
  noteEl: string
  firstRadio: { current: HTMLButtonElement | null }
  canSend: boolean
  note: string
  onSend: () => void
}) {
  // A lista do grupo: as opções na ordem do payload e Nenhuma destas por último.
  const t = useT()
  const picks: Pick[] = [...choice.data.options.map((o) => ({ kind: 'option' as const, id: o.id })), { kind: 'none' as const }]
  const keyOf = (p: Pick) => (p.kind === 'option' ? `o:${p.id}` : p.kind)
  const selected = draft.pick && draft.pick.kind !== 'text' ? keyOf(draft.pick) : ''
  const stop = radioTabStop(picks.map(keyOf), selected)
  const rows = useRef<(HTMLButtonElement | null)[]>([])
  rows.current.length = picks.length

  const choose = (p: Pick) => {
    if (p.kind !== 'text') setDraft(pickOption(draft, p))
  }
  // Setas só dentro do grupo com foco (WAI-ARIA radio): movem e escolhem. Letras ficam livres.
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = radioArrowTarget(e, index, picks.length)
    if (next === null) return
    e.preventDefault()
    e.stopPropagation()
    rows.current[next]?.focus()
    choose(picks[next]!)
  }

  return (
    <>
      <div className="genui-choice-options" role="radiogroup" aria-labelledby={questionEl}>
        {picks.map((p, i) => {
          const on = keyOf(p) === selected
          const option = p.kind === 'option' ? choice.data.options[i]! : null
          return (
            <div key={keyOf(p)} className="genui-choice-row-wrap">
              {p.kind === 'none' ? <div className="genui-choice-sep" role="none" /> : null}
              <button
                type="button"
                ref={(el) => { rows.current[i] = el; if (i === 0) firstRadio.current = el }}
                className={`genui-choice-row${p.kind === 'none' ? ' is-none' : ''}`}
                role="radio"
                aria-checked={on}
                tabIndex={i === stop ? 0 : -1}
                onClick={() => choose(p)}
                onKeyDown={(e) => onKeyDown(e, i)}
              >
                <span className="genui-choice-key" aria-hidden="true">{option ? optionKey(i) : '–'}</span>
                <span className="genui-choice-label" dir={option ? 'auto' : undefined}>{option ? option.label : t('conversa.escolha.nenhuma')}</span>
                <span className="genui-choice-radio" aria-hidden="true" />
              </button>
            </div>
          )
        })}
      </div>
      <div className="genui-choice-free">
        <label htmlFor={textEl}>{t('conversa.escolha.escrevaOutra')}</label>
        {/* Multilinha: Enter quebra a linha; só Enviar resposta envia. */}
        <textarea
          id={textEl}
          dir="auto"
          rows={2}
          value={draft.text}
          placeholder={t('conversa.escolha.placeholder')}
          aria-describedby={noteEl}
          onChange={(e) => setDraft(typeText(draft, e.target.value))}
        />
      </div>
      <div className="genui-choice-actions">
        <button type="button" className="styled-button primary" disabled={!canSend} aria-describedby={noteEl} onClick={onSend}>
          {t('conversa.escolha.enviar')}
        </button>
        <span className="genui-choice-note" id={noteEl}>{note}</span>
      </div>
    </>
  )
}

/** Opções como texto legível, sem controle: enviando, sem confirmação e estados finais. */
function ShownList({ choice, view }: { choice: ChoiceV1; view: ChoiceView }) {
  const t = useT()
  const shown = view.shown
  const tag = view.status === 'answered' ? (
    <span className="genui-choice-tag"><Icon name="checkmark" size={11} weight="semibold" className="genui-choice-tag-mark" />{t('conversa.escolha.suaResposta')}</span>
  ) : view.status === 'sending' ? (
    <span className="genui-choice-tag is-sending"><Spinner size={11} />{t('conversa.escolha.enviando')}</span>
  ) : view.status === 'unconfirmed' ? (
    <span className="genui-choice-tag is-sending"><Icon name="exclamationmark.circle" size={11} />{t('conversa.escolha.estado.semConfirmacao')}</span>
  ) : null
  const isShown = (p: Pick) => !!shown && (p.kind === 'none' ? shown.kind === 'none' : p.kind === 'option' && shown.kind === 'option' && shown.id === p.id)
  return (
    <ul className="genui-choice-list" aria-label={t('conversa.escolha.opcoes')}>
      {choice.data.options.map((o, i) => {
        const on = isShown({ kind: 'option', id: o.id })
        return (
          <li key={o.id} className={on ? 'is-picked' : undefined}>
            <span className="genui-choice-key" aria-hidden="true">{optionKey(i)}</span>
            <span className="genui-choice-label"><span className="genui-choice-sr">{t('conversa.escolha.opcaoSr', { letra: optionKey(i) })}</span>{o.label}</span>
            {on ? tag : null}
          </li>
        )
      })}
      <li className={`is-none${isShown({ kind: 'none' }) ? ' is-picked' : ''}`}>
        <span className="genui-choice-key" aria-hidden="true">–</span>
        <span className="genui-choice-label">{t('conversa.escolha.nenhuma')}</span>
        {isShown({ kind: 'none' }) ? tag : null}
      </li>
      {shown?.kind === 'text' ? (
        <li className="is-free is-picked">
          <span className="genui-choice-key" aria-hidden="true"><Icon name="pencil" size={11} /></span>
          <span className="genui-choice-label">
            <span className="genui-choice-free-label">{t('conversa.escolha.respostaEscritaRotulo')}</span>
            <span className="genui-choice-free-text" dir="auto">{shown.text}</span>
          </span>
          {tag}
        </li>
      ) : null}
    </ul>
  )
}
