// /clear da Open: a regra (resumir, guardar, apagar) é do Hub (`donnaOpenClear`); o app só pede, depois de o dono confirmar,
// e mostra o resultado. Aqui ficam o comando do menu `/` e a leitura do código de falha.
import { isOpenDesktop } from '../../../../shared/fork-capacidades.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { Bot } from '../../../../shared/models.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import type { SlashCommand } from '../../thread/slash-commands.ts'

export const RPC_LIMPAR = 'donnaOpenClear'

/** O comando /clear: só na edição Open e só em conversa com um bot (grupo não tem resumo). */
export function comandoLimpar(bot: Bot, idioma: AppLanguage): SlashCommand[] {
  if (typeof window === 'undefined' || !isOpenDesktop(window.codync) || bot.kind === 'group') return []
  return [{ name: 'clear', icon: 'trash', title: traduzir(idioma, 'contexto.limpar.comando'), detail: traduzir(idioma, 'contexto.limpar.detalhe'), warning: false, action: { kind: 'clear' } }]
}

export type FalhaLimpar = 'resumo' | 'memoria' | 'ocupada' | 'generico'

/** Código estável do Hub vira a falha que a tela traduz; qualquer outra coisa é genérica (nada foi apagado). */
export function falhaLimpar(erro: unknown): FalhaLimpar {
  const bruto = String((erro as { message?: unknown })?.message ?? erro)
  if (bruto.includes('clear_summary_failed')) return 'resumo'
  if (bruto.includes('clear_memory_failed')) return 'memoria'
  if (/open_chat_busy|context_busy/.test(bruto)) return 'ocupada'
  return 'generico'
}
