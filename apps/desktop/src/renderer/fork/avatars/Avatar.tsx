import { memo, useEffect, useId, useRef, useState, useSyncExternalStore } from 'react'
import type { AvatarMoodState } from '@shared/fork-avatar-mood'
import { UNAVAILABLE_TEXT, type Bot } from '@shared/models'
import { avatarMoodsFor, IDLE_MOOD } from '@shared/fork-avatar-mood'
import { conjuntoDoAvatar } from '@shared/fork-mexeriquinha-config'
import { DEFAULT_CONFIG, avatarDoBot, normalizarAvatarDoBot, type AvatarConfig, type BotAvatarConfig } from '@shared/fork-avatar-config'
import { BotFace, useBotMood, type Mood as FaceMood } from '../../lib/donnabot-avatars/src'
import { avatarColor, useReduceMotion } from '../../lib/theme'
import { CONFIG_BASE, renderSimples } from '../../lib/mexeriquinhas/index.ts'
import { estadoPorId, type EstadoId } from '../../lib/mexeriquinhas/estado.ts'
import { useSelector } from '../../lib/observable'
import { estadoDoBot, proximoSono, quadrosDoEstado } from '../../lib/mexeriquinhas-runtime.ts'
import { srcDoQuadroPelucia } from './mexeriquinhas-assets.ts'
import '../../lib/mexeriquinhas/mexeriquinhas.css'
import '../../lib/donnabot-avatars/src/styles/botface.css'
import './avatars.css'
import { traduzir } from '../../lib/fork-i18n/index.ts'

/** Compatibilidade de chamadas upstream; o desenho agora pertence à Mexerica. */
export type Mood = 'idle' | 'working' | 'needsInput' | FaceMood
const faceMood = (m: Mood): FaceMood => m === 'working' ? 'think' : m === 'needsInput' ? 'listen' : m
/** Rosto parado para tiles e listas (Novo bot): a frutinha da função ou o BotFace de antes. */
export function RostoParado({ config, size }: { config: AvatarConfig; size: number }) {
  const id = `mxq-tile-${useId().replace(/[^A-Za-z0-9_-]/g, '')}`
  const face = normalizarAvatarDoBot(config)
  // Estes tiles de função recebem apenas o formato legado; a equivalência é Simples.
  const aparencia = face.estilo === 'simples' ? face.aparencia : CONFIG_BASE
  return <span className="donnabot-avatar mxq-pausado" data-mexeriquinha="simples" aria-hidden="true" style={{ display: 'inline-flex', flex: 'none', width: size, height: size }}
    dangerouslySetInnerHTML={{ __html: renderSimples(aparencia, { size, id, label: '' }) }} />
}

const shapes: Record<string, AvatarConfig['shape']> = { blob: 'oval', pebble: 'pilula', squircle: 'cubo', tablet: 'tela', wedge: 'losango', hex: 'hexa', cloud: 'nuvem', teardrop: 'gota' }

export function placeholderConfig(shape: string, color: string): AvatarConfig {
  return { ...DEFAULT_CONFIG, shape: shapes[shape] ?? DEFAULT_CONFIG.shape, color: /^#[0-9a-fA-F]{6}$/.test(color) ? color : avatarColor(color) }
}

const subscribeVisibility = (listener: () => void) => {
  document.addEventListener('visibilitychange', listener)
  return () => document.removeEventListener('visibilitychange', listener)
}

/** Pausa também o CSS da biblioteca, além dos timers de vida automática. */
export const DonnabotAvatar = memo(function DonnabotAvatar({ config, size = 44, mood = 'idle', estado, label, animated = true }: {
  config?: BotAvatarConfig; size?: number; mood?: FaceMood; estado?: EstadoId | null; label: string; animated?: boolean
}) {
  const ref = useRef<HTMLSpanElement>(null)
  const id = `mxq-app-${useId().replace(/[^A-Za-z0-9_-]/g, '')}`
  const [visible, setVisible] = useState(false)
  const hidden = useSyncExternalStore(subscribeVisibility, () => document.hidden, () => true)
  const reduced = useReduceMotion()
  const running = animated && visible && !hidden && !reduced
  useEffect(() => {
    if (!ref.current) return
    const observer = new IntersectionObserver(([entry]) => setVisible(entry?.isIntersecting ?? false))
    observer.observe(ref.current)
    return () => observer.disconnect()
  }, [animated])
  const face = normalizarAvatarDoBot(config)
  const atual = estado === undefined ? estadoDoHumor(mood) : estado
  const visual = atual ?? 'livre'
  const nome = label ? `${label}, ${atual === null ? UNAVAILABLE_TEXT : estadoPorId(visual).nome}` : ''
  const classes = `donnabot-avatar${running ? '' : ' mxq-pausado mxp-pausado'}`
  // Um cartão 3D fora da vista não monta nenhuma img; estáticos visíveis usam só um quadro.
  const quadros = quadrosDoEstado(visual, running)
  const selo = estadoPorId(visual).selo
  return <span ref={ref} className={classes} aria-hidden={!label || undefined} data-paused={!running} data-mexeriquinha={face.estilo} data-avatar-state={atual ?? 'unavailable'} style={{ display: 'inline-flex', flex: 'none', width: size, height: size }}>
    {face.estilo === 'simples'
      ? <span dangerouslySetInnerHTML={{ __html: renderSimples(face.aparencia, { size, state: visual, id, label: nome }) }} />
      : <span className={`mxp mxp--${visual}${running ? '' : ' mxp--pausado'}`} role="img" aria-label={nome} style={{ width: size, height: size }}>
        <span className="mxp-fig">{visible && quadros.map((quadro, i) => <img key={quadro} className={`mxp-quadro mxp-quadro--${i}`} src={srcDoQuadroPelucia(conjuntoDoAvatar(face)!, size, quadro)} width={size} height={size} alt="" aria-hidden="true" draggable={false} loading="lazy" decoding="async" />)}</span>
        {selo && atual !== null ? <span className="mxp-selo" aria-hidden="true">{selo === 'x' ? '×' : selo}</span> : null}
      </span>}
  </span>
})

const estadoDoHumor = (mood: string): EstadoId => ['think', 'work', 'working'].includes(mood) ? 'trabalhando' : mood === 'listen' ? 'precisa-resposta' : 'livre'

/** Timer só na borda do sono. Visibilidade não altera a hora da última atividade. */
function useEstadoDoBot(bot: Pick<Bot, 'status' | 'lastAt' | 'createdAt' | 'startedAt' | 'pendingQuestions'>, atividade = 0, disponivel = true): EstadoId | null {
  const [, tick] = useState(0)
  const now = Date.now()
  const delay = disponivel ? proximoSono(bot, now, atividade) : null
  useEffect(() => {
    if (delay === null) return
    const timer = setTimeout(() => tick((v) => v + 1), Math.min(delay, 2_147_483_647))
    return () => clearTimeout(timer)
  }, [delay])
  return disponivel ? estadoDoBot(bot, now, atividade) : null
}

export function CharacterAvatar({ shape, color, tint, config, size = 44, mood = 'idle', state, label = traduzir('pt-BR', 'marca.avatar.padrao') }: {
  shape: string; color: string; tint?: string; config?: BotAvatarConfig; state?: AvatarMoodState; size?: number; mood?: Mood; dark?: boolean; label?: string
}) {
  const status = state?.runtimeStatus ?? 'unavailable'
  const projetado = status !== 'error' && status !== 'unavailable' && state?.precisaResposta ? 'needsInput' : status
  const remoto = useEstadoDoBot({ status: projetado, lastAt: state?.ultimaAtividade ?? 0, createdAt: 0, pendingQuestions: [] }, 0, !state?.indisponivel)
  // Sem identidade de bot, continua sendo o mascote legado do provedor, fora da fase 2a.
  if (!config && !state) return <span className="donnabot-avatar" data-paused="true"><BotFace config={placeholderConfig(shape, tint ?? color)} size={size} mood={faceMood(mood)} autoLife={false} label={label} /></span>
  const atual = state?.runtimeStatus ? remoto : estadoDoHumor(faceMood(mood))
  return <DonnabotAvatar config={config ?? placeholderConfig(shape, tint ?? color)} size={size} estado={atual} label={label} />
}

export function useAvatarMood(bot: Bot, animated: boolean): FaceMood {
  const store = avatarMoodsFor(bot)
  // MX-4 (C11): atrás de uma tela que cobre o chat, o humor congela com as demais assinaturas.
  const state = useSelector(store, (s) => s?.get(bot.id) ?? IDLE_MOOD)
  const { mood, setBase, play } = useBotMood('idle')
  useEffect(() => {
    const base = store ? state.base : bot.status === 'working' ? 'think' : bot.status === 'needsInput' ? 'listen' : 'idle'
    setBase(base)
    if (animated && state.action && Date.now() - state.at < 3000) play(state.action)
  }, [store, state, bot.status, animated, setBase, play])
  return animated || state.base === 'sleep' ? mood : 'idle'
}

/** O mesmo estado real em todos os tamanhos, inclusive quando a animação está pausada. */
export function useEstadoMexeriquinha(bot: Bot): EstadoId | null {
  const store = avatarMoodsFor(bot)
  const snapshot = useSelector(store, (s) => s?.get(bot.id) ?? IDLE_MOOD)
  return useEstadoDoBot(bot, snapshot.ultimaAtividade, !snapshot.indisponivel)
}

export function BotAvatar({ bot, size = 44, animated = true, state = false }: { bot: Bot; size?: number; animated?: boolean; state?: boolean }) {
  const atual = useEstadoMexeriquinha(bot)
  const face = <DonnabotAvatar config={avatarDoBot(bot) ?? normalizarAvatarDoBot(undefined, bot.name, bot.description)} size={size} estado={atual} animated={animated} label={bot.name} />
  return state ? <span className="mexeriquinha-state" title={atual === null ? UNAVAILABLE_TEXT : estadoPorId(atual).nome} style={{ display: 'inline-flex', width: size, height: size }}>{face}</span> : face
}
