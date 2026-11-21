// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useRef, useState } from 'react'
import { useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from '../settings/rotulo'
import type { IslandBot, IslandPermission } from '@shared/island'
import { editOrigin } from '@shared/composer-segments'
import { originDevice, originReport, type OriginDraft } from '@shared/guard-hub'
import { CharacterAvatar } from '../../components/Avatar'
import { Button, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ThinkingOrb } from '../../components/ThinkingOrb'
import { withAlpha } from '../../lib/theme'
import { ptBackend } from '../../lib/textos-pt'
import { useComposerInput } from '../../lib/use-composer-input'
import type { IslandModel } from './island-model'
import { useArrivals, useFlip, useSwap } from './island-motion'
import { AgentRow, badgeOf, BigCharacter, GREEN, KeyHint, MiniColumn, moodOf, RED, StateBadge, StatusDot, stepIcon } from './island-parts'

const veil = (color: string, alpha: number) => `radial-gradient(120% 90% at 50% 130%, ${withAlpha(color, alpha)}, ${withAlpha(color, 0)} 70%)`

/**
 * Closed over the notch: the bot in focus on the left wing; the others (or its state) on the
 * right. With no notch to hide, the middle says what's going on.
 */
export function Wings({ model, peek }: { model: IslandModel; peek: boolean }) {
  const t = useT()
  const focus = model.focus
  const others = model.others(2)
  const swap = useSwap(focus?.ref)
  const arrivals = useArrivals(others.map((b) => b.ref))
  const cluster = useRef<HTMLSpanElement>(null)
  useFlip(cluster)
  if (!focus) return null
  const label = focus.status === 'needsInput' ? t('navegacao.barra.principal.precisa') : t.plural('navegacao.ilha.trabalhando', model.activeCount)
  return (
    <div className={`island-wings ${peek ? 'peeking' : ''}`}>
      {model.screen.notch || peek ? null : <span className="island-wings-label">{label}</span>}
      <span className="island-wing-left">
        <span key={focus.ref} className={`island-wing-avatar ${swap}`}>
          <CharacterAvatar config={focus.avatarConfig} state={focus.avatarMood} label={focus.avatarLabel ?? t('navegacao.ilha.botAssistente', { bot: focus.name })} shape={focus.shape} color={focus.color} size={24} mood={moodOf(focus)} dark />
          {peek ? null : <StateBadge badge={badgeOf(focus)} small />}
        </span>
      </span>
      {peek ? null : (
        <span className="island-wing-right">
          {others.length ? (
            <span ref={cluster} className="island-cluster">
              {others.map((b) => (
                <span key={b.ref} data-flip={b.ref} className={`island-cluster-bot ${arrivals.has(b.ref) ? 'island-swap' : ''}`}>
                  <CharacterAvatar config={b.avatarConfig} state={b.avatarMood} label={b.avatarLabel ?? t('navegacao.ilha.botAssistente', { bot: b.name })} shape={b.shape} color={b.color} size={24} mood={moodOf(b)} dark />
                  {b.status === 'needsInput' ? <StatusDot bot={b} /> : null}
                </span>
              ))}
            </span>
          ) : focus.status === 'needsInput' ? (
            <span className="island-dot needs-you" />
          ) : (
            <ThinkingOrb size={14} color="#F5F6F8" />
          )}
        </span>
      )}
    </div>
  )
}

/** The bot in focus with its latest steps, and the other bots as pills. */
export function Overview({ model }: { model: IslandModel }) {
  const t = useT()
  const focus = model.focus
  const others = model.others(4)
  const swap = useSwap(focus?.ref)
  const arrivals = useArrivals(others.map((b) => b.ref))
  const pills = useRef<HTMLDivElement>(null)
  useFlip(pills)
  if (!focus) return null
  const steps = focus.steps.length ? focus.steps : [{ title: focus.detail, kind: 'other' }]
  return (
    <div className="island-row-cards">
      <button className={`island-card island-focus ${others.length ? '' : 'alone'}`} onClick={() => model.openBot(focus.ref)} title={t('navegacao.ilha.abrirConversa')}>
        <span key={focus.ref} className={`island-swap-slot ${swap}`}>
          <BigCharacter bot={focus} size={70} mood={moodOf(focus)} badge={badgeOf(focus)} />
          <span className="island-focus-text">
            <AgentRow color={focus.color} name={focus.name} subtitle={focus.backend} />
            {steps.map((step, i) => (
              <span key={`${i}-${step.title}`} className={`island-step ${i === steps.length - 1 ? 'current' : ''}`}>
                <Icon name={stepIcon(step.kind)} size={i === steps.length - 1 ? 13 : 12} scaled={false} />
                <span className="truncate" title={step.original ?? step.title}>{step.title}</span>
              </span>
            ))}
          </span>
        </span>
      </button>
      {others.length ? (
        <div ref={pills} className="island-card island-pills">
          {others.map((b) => (
            <button
              key={b.ref}
              data-flip={b.ref}
              className={`island-pill ${arrivals.has(b.ref) ? 'island-swap' : ''}`}
              style={{ background: withAlpha(b.color, 0.16), color: `color-mix(in srgb, ${b.color} 55%, white)` }}
              onClick={() => model.openBot(b.ref)}
              title={b.detail}
            >
              <span className="island-pill-bot">
                <CharacterAvatar config={b.avatarConfig} state={b.avatarMood} label={b.avatarLabel ?? t('navegacao.ilha.botAssistente', { bot: b.name })} shape={b.shape} color={b.color} size={24} mood={moodOf(b)} dark />
                <StatusDot bot={b} />
              </span>
              <span className="truncate">{b.name}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}

export function Empty({ model }: { model: IslandModel }) {
  const t = useT()
  const focus = model.focus
  // runtime-audiencia-01: um bot com Estado indisponível pode estar trabalhando; não afirmar que nada roda.
  const unknown = model.summary.bots.some((b) => b.status === 'unavailable')
  return (
    <div className="island-card island-empty">
      {focus ? <BigCharacter bot={focus} size={62} mood="idle" badge={null} /> : null}
      <span className="island-empty-text">
        <span className="island-title">{t(unknown ? 'navegacao.ilha.nadaMostrar' : 'navegacao.ilha.nadaRodando')}</span>
        <span className="island-subtitle">{unknown ? t('navegacao.ilha.indisponiveis') : focus ? t('marca.ilha.parados') : t('marca.ilha.crie')}</span>
      </span>
      {focus ? <Button onClick={() => model.setTab('message')}>{t('navegacao.ilha.mensagemBot')}</Button> : <Button onClick={() => model.newBot()}>{t('navegacao.janela.novoBot')}</Button>}
    </div>
  )
}

/** Deny on the left, the main allow on the right. */
const optionOrder = (kind: string) => ({ reject_always: 0, reject_once: 1, allow_always: 2, allow_once: 3 })[kind] ?? 1.5

export function Approval({ model, card }: { model: IslandModel; card: IslandPermission }) {
  const options = [...card.options].sort((a, b) => optionOrder(a.kind) - optionOrder(b.kind))
  const swap = useSwap(card.entryId)
  return (
    <div className="island-with-minis">
      {/* Auditoria D249, F6: sem o véu âmbar na base; o selo já diz "precisa de você". */}
      <div className="island-card island-alert" style={{ background: 'var(--island-card)' }}>
        <div key={card.entryId} className={`island-swap-slot ${swap}`}>
          <BigCharacter bot={card.bot} size={56} mood="needsInput" badge="needsInput" onOpen={() => model.openBot(card.ref)} />
          <div className="island-alert-text">
            <AgentRow color={card.bot.color} name={card.bot.name} subtitle={card.headline ? `${ptBackend(card.bot.backend)} · ${card.headline}` : ptBackend(card.bot.backend)} />
            {card.title ? <div className="island-code">{card.title}</div> : null}
            <div className="island-buttons">
              {options.map((o) => (
                <Button key={o.id} kind={o.kind === 'allow_once' ? 'primary' : 'secondary'} disabled={card.answering !== null} onClick={() => model.respond(card, o.id)}>
                  {card.answering === o.id ? <Spinner size={12} /> : null}
                  {o.label}
                  {o.kind === 'allow_once' ? <KeyHint keys="⌥Y" /> : o.kind === 'reject_once' ? <KeyHint keys="⌥N" /> : null}
                </Button>
              ))}
            </div>
          </div>
        </div>
      </div>
      <MiniColumn bots={model.others(3).filter((b) => b.ref !== card.ref)} onOpen={(ref) => model.openBot(ref)} />
    </div>
  )
}

/** A bot that just finished (for a moment) or failed (until dismissed). */
export function Notice({ model, bot, failed }: { model: IslandModel; bot: IslandBot; failed: boolean }) {
  const t = useT()
  const swap = useSwap(`${failed}:${bot.ref}`)
  return (
    <div className="island-with-minis">
      <div className="island-card island-alert" style={{ background: `${veil(failed ? RED : GREEN, failed ? 0.5 : 0.42)}, var(--island-card)` }}>
        <div key={`${failed}:${bot.ref}`} className={`island-swap-slot ${swap}`}>
          <BigCharacter bot={bot} size={58} mood="idle" badge={failed ? 'failed' : 'finished'} onOpen={() => model.openBot(bot.ref)} />
          <div className="island-alert-text">
            <AgentRow color={bot.color} name={bot.name} subtitle={failed ? ptBackend(bot.backend) : t('navegacao.ilha.terminou', { motor: ptBackend(bot.backend) })} />
            {failed ? (
              <>
                <div className="island-title">{comNos(t('navegacao.ilha.parou', { bot: no('bot') }), { bot: <bdi dir="auto">{bot.name}</bdi> })}</div>
                <div className="island-error">{bot.error ?? bot.detail}</div>
              </>
            ) : (
              <div className="island-title truncate">{bot.detail}</div>
            )}
            <div className="island-buttons start">
              <Button onClick={() => model.openBot(bot.ref)}>{t('navegacao.ilha.abrirConversa')}</Button>
              <Button kind="secondary" onClick={() => model.dismissNotice()}>{t('navegacao.ilha.ok')}</Button>
            </div>
          </div>
        </div>
      </div>
      <MiniColumn bots={model.others(3).filter((b) => b.ref !== bot.ref)} onOpen={(ref) => model.openBot(ref)} />
    </div>
  )
}

/** Writes to a bot without opening Codync: the chip picks the bot, Enter sends, Esc closes. */
export function Message({ model }: { model: IslandModel }) {
  const t = useT()
  const [text, setText] = useState('')
  const origin = useRef<OriginDraft | undefined>(undefined)
  const latest = useRef(text)
  latest.current = text
  const { input, bind } = useComposerInput<HTMLInputElement>()
  const bot = model.messageBot
  const swap = useSwap(bot?.ref)
  if (!bot) return null
  return (
    <div className="island-with-minis">
      <div className="island-card island-message">
        <span key={bot.ref} className={`island-message-bot ${swap}`}>
          <CharacterAvatar config={bot.avatarConfig} state={bot.avatarMood} label={bot.avatarLabel ?? t('navegacao.ilha.botAssistente', { bot: bot.name })} shape={bot.shape} color={bot.color} size={44} mood={moodOf(bot)} dark />
        </span>
        <div className="island-message-body">
          <div className="island-agent">
            <button
              key={bot.ref}
              className={`island-chip ${swap}`}
              style={{ background: withAlpha(bot.color, 0.16), color: `color-mix(in srgb, ${bot.color} 55%, white)` }}
              onClick={() => model.nextMessageBot()} disabled={model.sending}
              title={t('navegacao.ilha.outroBot')}
            >
              <CharacterAvatar config={bot.avatarConfig} state={bot.avatarMood} label={bot.avatarLabel ?? t('navegacao.ilha.botAssistente', { bot: bot.name })} shape={bot.shape} color={bot.color} size={24} mood={moodOf(bot)} dark />
              <bdi dir="auto">{bot.name}</bdi>
              <Icon name="chevron.down" size={8} weight="bold" scaled={false} />
            </button>
            {model.sendState !== 'idle'
              ? <span className="island-agent-subtitle is-aviso" role="status">{t(model.sendState === 'bloqueado' ? 'conversa.compositor.senha.envioBloqueado' : 'conversa.compositor.senha.semConfirmacao')}</span>
              : <span className="island-agent-subtitle">{bot.backend} · <bdi dir="ltr">{bot.folder}</bdi></span>}
          </div>
          <form
            className="island-field"
            onSubmit={(e) => {
              e.preventDefault()
              if (model.sending) return
              // MX-214: só limpa o campo com aceite explícito da conversa, e só se o campo ainda é o texto enviado (durante
              // a espera ele fica congelado); barrado ou sem confirmação, o texto fica.
              const enviado = text
              void model.send(enviado, originReport(enviado, origin.current, originDevice())).then((aceito) => {
                if (!aceito || latest.current !== enviado) return
                setText('')
                origin.current = undefined
              })
            }}
          >
            <input
              ref={bind}
              autoFocus
              aria-label={t('navegacao.ilha.mensagemPara', { bot: bot.name })}
              placeholder={t('navegacao.ilha.mensagemParaPlaceholder', { bot: bot.name })}
              value={text}
              readOnly={model.sending}
              onChange={(e) => {
                const { kind, range } = input.change(text, e.target.value)
                origin.current = editOrigin(text, e.target.value, origin.current, kind, range)
                setText(e.target.value)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (input.composing || e.nativeEvent.isComposing || e.keyCode === 229)) e.preventDefault()
                if (e.key === 'Escape') model.close()
              }}
            />
            <button type="submit" className="island-send" aria-label={t('navegacao.ilha.enviar')} title={t('navegacao.ilha.enviar')} disabled={!text.trim() || model.sending}>
              <Icon name="arrow.up" size={12} weight="bold" scaled={false} />
            </button>
          </form>
        </div>
      </div>
      <MiniColumn bots={model.others(3).filter((b) => b.ref !== bot.ref)} onOpen={(ref) => model.openBot(ref)} />
    </div>
  )
}
