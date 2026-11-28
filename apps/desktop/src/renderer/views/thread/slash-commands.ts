// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { isAppLanguage } from '../../../shared/fork-idioma.ts'
import { traduzir, tradutor, type Tradutor } from '../../lib/fork-i18n/index.ts'
// The composer's `/` commands, listed while the draft is just `/partial`: Fast mode runs here
// through the host's `updateBot` (a new session is in the bot's menu); a skill fills in `/<id> `, and the host turns that message into "read this skill first"
// (host/src/market/skills.rs `expand_command`). A group lists its members' commands: Fast mode
// for all of them, and every skill one of them has. docs/features/marketplace.md, "Commands".
import { fastModeDetail, fastModeShown, fastModeState, groupFastMode, type FastModeState } from '../../../shared/fast-mode.ts'
import type { Bot } from '../../../shared/models.ts'

export type SlashAction = { kind: 'fast' } | { kind: 'skill'; id: string } | { kind: 'newTab' }

export interface SlashCommand {
  /** What follows the `/`. */
  name: string
  icon: string
  title: string
  detail: string
  warning: boolean
  action: SlashAction
}

/** The command name being typed after a leading `/`, before any space; otherwise null. */
export function slashQuery(draft: string): string | null {
  return /^\/\S*$/.test(draft) ? draft.slice(1) : null
}

/** The bot's built-in commands; a group's Fast mode is its members' (`groupFastMode`). */
function builtIn(bot: Bot, members: Bot[], canNewTab = false): SlashCommand[] {
  // O estado de idiomas sincroniza lang no documento; sem DOM, vale o fallback da base.
  const lang = typeof document === 'undefined' ? null : document.documentElement.lang
  const idioma = isAppLanguage(lang) ? lang : 'pt-BR'
  const t = tradutor(idioma)
  const fast = bot.kind === 'group' ? groupFastMode(t, members) : ownFastMode(t, bot)
  const commands: SlashCommand[] = fast ? [{ name: 'fast', icon: 'bolt', title: traduzir(idioma, 'conversa.fastMode'), detail: fast.detail, warning: fast.state === 'refused', action: { kind: 'fast' } }] : []
  if (canNewTab && bot.kind !== 'group') commands.push({ name: NEW_TAB, icon: 'plus', title: traduzir(idioma, 'sessions.newTab.title'), detail: traduzir(idioma, 'sessions.newTab.detail'), warning: false, action: { kind: 'newTab' } })
  return commands
}

const NEW_TAB = 'nova-aba'

/** `/nova-aba <texto>`: o texto a enviar na aba nova ('' sem texto); null se o rascunho não é esse comando. */
export function newTabText(draft: string): string | null {
  const m = /^\/nova-aba(?:\s+([\s\S]*))?$/.exec(draft.trim())
  return m ? (m[1] ?? '').trim() : null
}

/** A bot's own Fast mode, while it shows it. */
function ownFastMode(t: Tradutor, bot: Bot): { state: FastModeState; detail: string } | null {
  return fastModeShown(bot) ? { state: fastModeState(bot), detail: fastModeDetail(t, bot) } : null
}

/**
 * Built-in commands, then the bot's skills (ids in `bot.skills`; a group's `members` have them),
 * whose name or title holds `query`. `showSkills` off (DON-907, Ajustes → Conversa → "Skills no menu /"):
 * only the built-in commands; a typed `/<id> ` still reaches the host, which expands it.
 */
export function slashCommands(bot: Bot | null, query: string, installedSkills: { id: string; name: string; description: string }[], members: Bot[] = [], showSkills = true, canNewTab = false): SlashCommand[] {
  if (!bot) return []
  const enabled = !showSkills ? [] : bot.kind === 'group' ? members.flatMap((m) => m.skills) : bot.skills
  const skills = installedSkills
    .filter((s) => enabled.includes(s.id))
    .map((s): SlashCommand => ({ name: s.id, icon: 'book.pages', title: s.name, detail: s.description, warning: false, action: { kind: 'skill', id: s.id } }))
  const q = query.toLowerCase()
  return [...builtIn(bot, members, canNewTab), ...skills].filter((c) => c.name.includes(q) || c.title.toLowerCase().includes(q))
}

/** A draft that is just a built-in command (`/fast`): sending it runs the command. */
export function builtInCommand(bot: Bot | null, draft: string, members: Bot[] = [], canNewTab = false): SlashCommand | null {
  const name = slashQuery(draft.trim())
  return bot && name !== null ? (builtIn(bot, members, canNewTab).find((c) => c.name === name) ?? null) : null
}

/**
 * What Return does with `picked` highlighted: run it, except a skill whose name is already
 * typed out, which sends.
 */
export function returnRuns(picked: SlashCommand, draft: string): boolean {
  return picked.action.kind !== 'skill' || draft !== `/${picked.name}`
}
