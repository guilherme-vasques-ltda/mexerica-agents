import type { Bot } from '../../shared/models.ts'
import { INATIVIDADE_MS, resolveState, type EstadoId } from './mexeriquinhas/estado.ts'
import type { QUADROS } from './mexeriquinhas/render.ts'

export type QuadroPelucia = typeof QUADROS[number]
type SinaisBot = Pick<Bot, 'status' | 'lastAt' | 'createdAt' | 'startedAt' | 'pendingQuestions'>

/** Só fatos do Hub/entradas; render, leitura e conexão não reiniciam o relógio. */
export function ultimaAtividadeDoBot(bot: SinaisBot, atividade = 0): number | undefined {
  const tempos = [bot.lastAt, bot.createdAt, bot.startedAt, atividade]
    .filter((at): at is number => typeof at === 'number' && Number.isFinite(at) && at > 0)
  return tempos.length ? Math.max(...tempos) : undefined
}

/** null preserva a falta de informação de runtime, sem inventar um sexto estado do avatar. */
export function estadoDoBot(bot: SinaisBot, now: number, atividade = 0): EstadoId | null {
  if (bot.status === 'unavailable') return null
  return resolveState({
    erro: bot.status === 'error',
    precisaResposta: bot.status === 'needsInput' || (bot.pendingQuestions?.length ?? 0) > 0,
    ocupado: bot.status === 'working',
    ultimaAtividade: ultimaAtividadeDoBot(bot, atividade),
  }, now)
}

/** Um único despertar na borda estrita de 8h, sem polling por avatar. */
export function proximoSono(bot: SinaisBot, now: number, atividade = 0): number | null {
  if (estadoDoBot(bot, now, atividade) !== 'livre') return null
  const ultima = ultimaAtividadeDoBot(bot, atividade)
  return ultima === undefined ? null : Math.max(1, ultima + INATIVIDADE_MS + 1 - now)
}

/** Não baixa quadros de outros estados, nem quadros animados quando parado. */
export function quadrosDoEstado(estado: EstadoId, animado: boolean): readonly QuadroPelucia[] {
  const quadros: readonly QuadroPelucia[] = estado === 'livre' ? ['livre-1', 'livre-2', 'piscar']
    : estado === 'trabalhando' ? ['trabalhando-1', 'trabalhando-2']
    : estado === 'dormindo' ? ['dormindo-1', 'dormindo-2'] : [estado]
  return animado ? quadros : quadros.slice(0, 1)
}
