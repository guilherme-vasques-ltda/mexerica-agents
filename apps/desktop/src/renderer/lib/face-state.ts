// Barra05 (dono, 05/11/2026; docs/reference/redesenho/components/TeamGroup): o estado do bot vai no próprio rosto, sem
// pontinhos. Quatro estados, cada um com a pose da biblioteca (`BaseMood`) e um movimento leve no quadro em CSS
// (components/avatar.css): Livre parado, Trabalhando balança, Aguardando resposta sacode, Dormindo respira com um "z".
import type { Bot } from '../../shared/models.ts'

export type FaceState = 'idle' | 'work' | 'listen' | 'sleep'

export const FACE_STATE_LABEL: Record<FaceState, string> = { idle: 'Livre', work: 'Trabalhando', listen: 'Aguardando resposta', sleep: 'Dormindo' }

/**
 * Pedido seu vence trabalho, que vence o sono. Dormindo é só o sono que já existe (bot desligado, parado ou sem
 * conexão: a pose `sleep` da biblioteca); não há regra de horas parado (coordenação, 05/11/2026).
 */
export function faceState(bot: Pick<Bot, 'status'>, mood: string): FaceState {
  if (bot.status === 'needsInput') return 'listen'
  if (bot.status === 'working') return 'work'
  if (mood === 'sleep' || mood === 'drowsy') return 'sleep'
  return 'idle'
}
