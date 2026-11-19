import type { VoiceCallEngine } from '@shared/analytics'
import { isWorking } from '@shared/models'
import { createContext, useContext, useEffect, useRef, useState } from 'react'
import { BotAvatar } from '../../components/Avatar'
import { Icon } from '../../components/Icon'
import { Sheet } from '../../components/Overlay'
import { Text } from '../../components/Text'
import { useModel } from '../../lib/observable'
import { botAccent } from '@shared/fork-avatar-config'
import { useDark, usePixelRatio, useReduceMotion, withAlpha } from '../../lib/theme'
import { analytics } from '../../store/analytics'
import { useStore } from '../../store/context'
import { VozPage } from '../fork/voz/VozPage'
import { LimitesChamada, TEXTOS_LIMITE, type EstadoLimites } from '../fork/voz/limites-chamada'
import { criarMotor, depoisDaFalha, nomeDoMotor, planoDeQueda, planoDoMotor, type PlanoMotor } from '../fork/voz/motor-chamada'
import { TEXTOS_CHAMADA } from '../fork/voz/textos-chamada'
import { ESTADO_CHAMADA, estadoDaBarra, tempoMostrado } from '../fork/voz/barra-chamada'
import { chavesDoAparelho } from '../fork/voz/estado-aparelho'
import { fonteDaChave } from '../fork/voz/voz-model'
import { chosenListen } from '../settings/voice-settings'

/**
 * Opens the window's own Settings at Voice chat; without it (no Settings screen around) the
 * call bar shows the voice settings in their own sheet.
 */
export const OpenVoiceSettings = createContext<(() => void) | null>(null)
import { countVoiceUsage, MIC_DENIED, onDeviceAvailable, onDeviceUnavailableReason, providerName, VoiceEngine, VoiceSettings, type VoiceProvider as VozProvider, type VoiceStatus } from './voice'
import { watchVoiceCall } from './voice-call'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { textosVoz } from '../fork/voz/frases'
import type { Mensagem } from '../fork/bot-settings/mensagem'
import './call.css'
import { bloqueioSenha, destinoSenha } from '../../store/fork-senha-bloqueio'

type CallProps = {
  botId: string
  onSpeaking: (speaking: boolean) => void
  interrupt: { current: (() => void) | null }
  onEnd: () => void
}

/**
 * A voice call with a bot, as Grok Bot does it: a floating bar over the chat, which stays
 * readable underneath. What you say goes out as ordinary messages, its final replies are read
 * aloud, and the chat gets a "Voice chat · 00:16" line when the call ends.
 * voz-03: "Ligar de novo" depois de um limite é uma chamada nova, com limites novos, só por clique.
 */
export function CallView(props: CallProps) {
  const [call, setCall] = useState(0)
  return <CallSession key={call} {...props} onRedial={() => setCall((n) => n + 1)} />
}

function CallSession({ botId, onSpeaking, interrupt, onEnd, onRedial }: CallProps & { onRedial: () => void }) {
  const store = useStore()
  const [session, setSession] = useState<VoiceEngine | null>(null)
  useModel(session)
  const t = useT()
  const idioma = useIdioma()
  /** One line under the bar, e.g. why the call fell back to another voice. D507: data, translated when drawn. */
  const [notice, setNotice] = useState<Mensagem[] | null>(null)
  /** No engine could start at all. */
  const [blocked, setBlocked] = useState<Mensagem[] | null>(null)
  const [settings, setSettings] = useState(false)
  const openVoiceSettings = useContext(OpenVoiceSettings)
  const engine = useRef<VoiceEngine | null>(null)
  /** When the current cloud engine started, for this month's minutes. */
  const cloudStartedAt = useRef<number | null>(null)
  const closed = useRef(false)
  /** What carries the current engine's audio, and whether the call has been counted (once its first engine runs). */
  const engineName = useRef<VoiceCallEngine>('on_device')
  const counted = useRef(false)
  const startedAt = useRef(Date.now())
  /** D249 F1: when finish() ended the call, for any reason: the bar's clock stops there. */
  const [endedAt, setEndedAt] = useState<number | null>(null)
  // voz-03 (S8): one clock per call, from the start, kept across fallbacks and reconnections.
  const [limit, setLimit] = useState<EstadoLimites>({ aviso: null, fim: null })
  const limits = useRef<LimitesChamada | null>(null)
  limits.current ??= new LimitesChamada(Date.now, setLimit)
  const bot = store.bots.get(botId)
  const color = bot ? botAccent(bot) : null

  const countCloudMinutes = () => {
    const provider = engine.current?.provider
    if (!provider || cloudStartedAt.current === null) return
    countVoiceUsage((Date.now() - cloudStartedAt.current) / 1000, provider)
    cloudStartedAt.current = null
  }

  /** Idempotent: stops every part of the call and logs it once. */
  const finish = () => {
    if (closed.current) return
    closed.current = true
    const at = Date.now()
    setEndedAt(at)
    limits.current?.parar()
    engine.current?.end()
    countCloudMinutes()
    onSpeaking(false)
    interrupt.current = null
    store.logCall(botId, Math.floor((at - startedAt.current) / 1000))
  }

  const send = (text: string) => {
    // MX-214: com o aviso de senha colada aberto neste bot, a fala não é enviada (BotStore.send também recusa).
    if (bloqueioSenha.bloqueado(destinoSenha(store.computer.id, botId))) return store.avisarEnvioBloqueado(botId)
    limits.current?.conversou()
    store.send(text, botId, { sourceKind: 'voice' })
  }

  /** Starts the engine the two choices of this device call for (fork/voz/motor-chamada); never a paid one nobody chose. */
  const begin = (plan: PlanoMotor) => {
    if (closed.current) return
    if ('chavePendente' in plan && plan.chavePendente) {
      if (plan.tipo === 'sem-voz') return void setBlocked([TEXTOS_CHAMADA.falhaSemQueda])
      setNotice([TEXTOS_CHAMADA.quedaLocal])
    }
    if (plan.tipo === 'sem-voz') return void setBlocked(TEXTOS_CHAMADA.semVoz(onDeviceUnavailableReason()))
    if (plan.tipo === 'sem-ouvir') return void setBlocked([TEXTOS_CHAMADA.semOuvir])
    if (plan.tipo === 'so-tempo-real') return void semTempoReal(plan.provedores)
    // voz-04: a parte que a chave deste Mac ainda não cobre segue no próprio Mac, com aviso.
    if ('transicao' in plan && plan.transicao) setNotice([TEXTOS_CHAMADA.transicao(plan.transicao)])
    const next = criarMotor(plan, { store, botId, send })
    const cancelarPedidos = next.onClosing
    next.onClosing = () => { cancelarPedidos?.(); limits.current?.parar() }
    next.onEndCall = () => { finish(); onEnd() }
    engineName.current = nomeDoMotor(plan)
    cloudStartedAt.current = next.provider ? Date.now() : null
    // Picks up the computer's newest models for the next call; this one starts right away.
    if (next.provider) void rememberStatus()
    next.onUserSpoke = () => limits.current?.conversou()
    let stopWatching: (() => void) | null = null
    next.onActivityChanged = (active) => {
      if (active && !stopWatching) {
        stopWatching = watchVoiceCall(store, botId, { speak: (t) => next.speak(t), announce: (t) => next.announce(t) })
      } else if (!active && stopWatching) {
        stopWatching()
        stopWatching = null
      }
    }
    engine.current = next
    setSession(next)
    void next.start()
  }

  /** voz-04 até o 05b, sem Ditado: a segunda saída (limpar a chave daqui) só se o Hub do bot tem chave. */
  const semTempoReal = async (provedores: VozProvider[]) => {
    setBlocked([TEXTOS_CHAMADA.soTempoReal(null)])
    const status = await rememberStatus()
    if (closed.current || !status || !provedores.every((p) => fonteDaChave(status, p) === 'hub')) return
    setBlocked([TEXTOS_CHAMADA.soTempoReal(store.hostName)])
  }

  const rememberStatus = async () => {
    try {
      const status = await (await store.ready()).call<VoiceStatus>('voiceStatus')
      VoiceSettings.remember(status)
      return status
    } catch {
      return null
    }
  }

  useEffect(() => {
    // voz-04: quem paga depende das chaves deste aparelho; sem saber, a chamada não liga.
    void chavesDoAparelho().then((aparelho) => {
      if (closed.current) return
      if (!aparelho) return void setBlocked([TEXTOS_CHAMADA.semEstadoDasChaves])
      const live = VoiceSettings.provider()
      begin(planoDoMotor({ aoVivo: live, fala: live ? VoiceSettings.isSpeechMode(live) : false, ouvir: chosenListen(), ditado: onDeviceAvailable(), aparelho }))
    })
    return finish
  }, [])

  // The limits' own clock (absolute deadlines, rescheduled whenever one moves), also checked when the window comes back.
  useEffect(() => {
    const clock = limits.current!
    const stop = clock.iniciarRelogio({ set: (fn, ms) => setTimeout(fn, ms), clear: (t) => clearTimeout(t as ReturnType<typeof setTimeout>) })
    const check = () => void clock.conferir()
    document.addEventListener('visibilitychange', check)
    window.addEventListener('focus', check)
    return () => {
      stop()
      document.removeEventListener('visibilitychange', check)
      window.removeEventListener('focus', check)
    }
  }, [])

  useEffect(() => {
    if (limit.fim) finish()
  }, [limit.fim])

  const phase = session?.phase ?? null
  const working = bot ? isWorking(bot) : false
  // The bot talking or working holds the no-conversation clock, never the 30-minute one.
  useEffect(() => limits.current?.botOcupado(phase === 'speaking' || working), [phase, working])

  useEffect(() => {
    if (!session || closed.current) return
    onSpeaking(phase === 'speaking')
    interrupt.current = phase === 'speaking' ? () => session.interrupt() : null
    if (session.isActive && !counted.current) {
      counted.current = true
      analytics.track({ event: 'voice_call_started', engine: engineName.current })
    }
    // A cloud engine that can't start or drops mid-call hands over to on-device voice, free.
    // voz-03: without it (Windows/Linux), the call says so and ends; "Ligar de novo" is a click away.
    const next = phase === 'failed' ? depoisDaFalha(!!session.provider, onDeviceAvailable()) : 'nada'
    if (next === 'queda') {
      session.end() // idempotent; closes the failed engine's session before the free one starts
      countCloudMinutes()
      setNotice([TEXTOS_CHAMADA.quedaLocal])
      begin(planoDeQueda(true))
    } else if (next === 'fim') {
      limits.current?.parar()
      finish()
    }
  }, [session, phase])

  const ended = limit.fim !== null
  const speaking = !ended && phase === 'speaking'
  const muted = session?.muted === true
  // D249 F1: the state in a word and the call's own clock (from its start; a fallback or engine swap keeps it).
  const statusText = t(ESTADO_CHAMADA[estadoDaBarra({ encerrada: ended, fase: phase, mudo: muted, trabalhando: working })])

  // Windows/Linux (no free fallback): the provider failed; say what to do, not the provider's words.
  const semQueda = phase === 'failed' && depoisDaFalha(!!session?.provider, onDeviceAvailable()) === 'fim'
  const falha = session?.falha ?? null
  const microfone = !!falha && 'chave' in falha && falha.chave === MIC_DENIED
  const failed: Mensagem[] | null = phase === 'failed' ? (semQueda && !microfone ? [TEXTOS_CHAMADA.falhaSemQueda] : falha ? [falha] : null) : null
  const failure = ended ? null : (blocked ?? failed)
  const textoFalha = failure ? textosVoz(t, failure) : null
  const textoAviso = notice ? textosVoz(t, notice) : null

  return (
    <div className="call idioma-superficie" {...superficieIdioma(idioma)}>
      <div className="call-bar">
        <button
          className={`call-avatar ${speaking ? 'speaking' : ''}`}
          disabled={!speaking}
          aria-label={speaking ? t('voz.chamada.interromper') : (bot?.name ?? '')}
          title={speaking ? t('voz.chamada.interromper') : undefined}
          onClick={() => session?.interrupt()}
        >
          {bot ? <BotAvatar bot={bot} size={34} /> : <span className="call-avatar-empty" />}
        </button>
        <CallLevelDots engine={ended ? null : session} working={!ended && working} color={color} label={statusText} />
        <div className="call-state" title={bot ? `${bot.name} · ${statusText}` : undefined}>
          <span className="call-state-line">
            {/* Only the state is announced, when it changes; the clock isn't read every second. */}
            <Text size="callout" weight="semibold" aria-live="polite">{statusText}</Text>
            <CallClock since={startedAt.current} until={endedAt} />
          </span>
          {/* Audio leaves the computer: say where. The line below is also where the voice's source goes (proposta voz, quadro 04). */}
          {!ended && session?.provider ? (
            <Text size="caption2" weight="semibold" tone="secondary" aria-label={t('voz.chamada.vozDe', { provedor: providerName(session.provider) })}>
              {providerName(session.provider)}
            </Text>
          ) : null}
        </div>
        <button className="call-round" title={t('voz.chamada.ajustes')} aria-label={t('voz.chamada.ajustes')} onClick={() => (openVoiceSettings ? openVoiceSettings() : setSettings(true))}>
          <Icon name="gearshape" size={16} weight="medium" scaled={false} />
        </button>
        <button
          className={`call-round ${muted ? 'on' : ''}`}
          title={muted ? t('voz.chamada.ativarMic') : t('voz.chamada.silenciarMic')}
          aria-label={muted ? t('voz.chamada.ativarMic') : t('voz.chamada.silenciarMic')}
          disabled={ended}
          onClick={() => session && (session.muted = !session.muted)}
        >
          <Icon name={muted ? 'mic.slash' : 'mic'} size={16} weight="medium" scaled={false} />
        </button>
        <button className="call-round end" title={t('voz.chamada.encerrar')} aria-label={t('voz.chamada.encerrar')} onClick={onEnd}>
          <Icon name="xmark" size={16} weight="semibold" scaled={false} />
        </button>
      </div>
      {limit.fim ? (
        <div key="ended" className="call-line call-line-action" role="status">
          <Text tone="secondary">{t(limit.fim === 'teto' ? TEXTOS_LIMITE.fimTeto : TEXTOS_LIMITE.fimSilencio)}</Text>
          <button className="call-line-button" onClick={onRedial}>{t(TEXTOS_LIMITE.ligarDeNovo)}</button>
        </div>
      ) : failure && semQueda && !blocked ? (
        <div key="failure-redial" className="call-line call-line-action" role="status">
          <Text tone="danger" dir="auto">{textoFalha}</Text>
          <button className="call-line-button" onClick={onRedial}>{t(TEXTOS_LIMITE.ligarDeNovo)}</button>
        </div>
      ) : failure ? (
        <Text as="div" key="failure" tone="danger" className="call-line" dir="auto">
          {textoFalha}
        </Text>
      ) : limit.aviso === 'silencio' ? (
        <div key="idle" className="call-line call-line-action" role="status">
          <Text tone="secondary">{t(TEXTOS_LIMITE.avisoSilencio)}</Text>
          <button className="call-line-button" onClick={() => limits.current?.continuar()}>{t(TEXTOS_LIMITE.continuar)}</button>
        </div>
      ) : limit.aviso === 'teto' ? (
        <Text as="div" key="cap" tone="secondary" className="call-line" role="status">
          {t(TEXTOS_LIMITE.avisoTeto)}
        </Text>
      ) : textoAviso ? (
        <Text as="div" key={textoAviso} tone="secondary" className="call-line">
          {textoAviso}
        </Text>
      ) : null}
      <Sheet open={settings} onClose={() => setSettings(false)} language={superficieIdioma(idioma)}>
        <VozPage reserva={store} folha />
      </Sheet>
    </div>
  )
}

/** D249 F1: the time since the call began (not the engine): a fallback or an engine swap keeps counting. */
function CallClock({ since, until }: { since: number; until: number | null }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (until !== null) return
    const tick = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(tick)
  }, [until])
  // Outside the live region (not read every second), but reachable by the screen reader when the person looks.
  return <Text size="callout" tone="secondary" tabular className="call-time"><span aria-hidden="true">· </span><bdi dir="ltr">{tempoMostrado(since, now, until)}</bdi></Text>
}

const DOTS_WIDTH = 120
const DOTS_HEIGHT = 16

/**
 * The dotted line in the call bar: it follows your voice while listening, ripples while the bot
 * talks, breathes while it works, and rests as faint dots otherwise.
 */
function CallLevelDots({ engine, working, color, label }: { engine: VoiceEngine | null; working: boolean; color: string | null; label: string }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const dark = useDark()
  const reduceMotion = useReduceMotion()
  const scale = usePixelRatio()
  const live = useRef({ engine, working, color, dark, reduceMotion })
  live.current = { engine, working, color, dark, reduceMotion }

  useEffect(() => {
    const el = canvas.current
    const ctx = el?.getContext('2d')
    if (!el || !ctx) return
    el.width = DOTS_WIDTH * scale
    el.height = DOTS_HEIGHT * scale
    let frame = 0
    const draw = (now: number) => {
      const { engine, working, color, dark, reduceMotion } = live.current
      const t = reduceMotion ? 0 : now / 1000
      const speaking = engine?.phase === 'speaking'
      const level = engine?.level ?? 0
      const accent = dark ? '#ffffff' : '#000000'
      const tint = speaking ? accent : (color ?? accent)
      ctx.setTransform(scale, 0, 0, scale, 0, 0)
      ctx.clearRect(0, 0, DOTS_WIDTH, DOTS_HEIGHT)
      const spacing = 8
      const count = Math.max(1, Math.floor(DOTS_WIDTH / spacing))
      for (let i = 0; i < count; i++) {
        const x = i * spacing + spacing / 2
        const wave = (Math.sin(i * 0.55 - t * 6) + 1) / 2
        const amount = speaking ? 0.5 + 0.5 * wave : level > 0.05 ? Math.max(0.2, level) * (0.45 + 0.55 * wave) : working ? 0.25 * ((Math.sin(t * 2.4) + 1) / 2) : 0.08
        const height = 4 + amount * (DOTS_HEIGHT - 4)
        ctx.fillStyle = withAlpha(tint, 0.5 + 0.5 * amount)
        ctx.beginPath()
        ctx.roundRect(x - 1.25, (DOTS_HEIGHT - height) / 2, 2.5, height, 1.25)
        ctx.fill()
      }
      frame = requestAnimationFrame(draw)
    }
    frame = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(frame)
  }, [scale])

  return <canvas ref={canvas} role="img" aria-label={label} className="call-dots" style={{ width: DOTS_WIDTH, height: DOTS_HEIGHT }} />
}
