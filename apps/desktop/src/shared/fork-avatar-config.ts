import { avatarDoBot, equivalenciaAvatar, validMexeriquinha, projetarAvatarLegado, type BotAvatarConfig } from './fork-mexeriquinha-config.ts'
export * from './fork-mexeriquinha-config.ts'
// D115: adaptação do fork; os arquivos src da biblioteca permanecem intactos.
import { ROLES, configFromRole } from '../renderer/lib/donnabot-avatars/src/core/data.ts'
import type { AvatarConfig } from '../renderer/lib/donnabot-avatars/src/core/types.ts'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import schema from '../renderer/lib/donnabot-avatars/schema/avatar-config.schema.json' with { type: 'json' }
import data from './fork-avatar-data.json' with { type: 'json' }

export type { AvatarConfig }
export { ROLES, configFromRole }
export const DEFAULT_CONFIG = data.defaultConfig as AvatarConfig
const folded = (s: string) => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
const aliases = (r: typeof ROLES[number], botName: boolean) => [...(botName ? [r.id, r.name] : []), r.role, ...data.roleAliases[r.id as keyof typeof data.roleAliases]].map(folded)
const contains = (text: string, word: string) => ` ${text} `.includes(` ${word} `)

export function configForRole(name = '', description = '', legacy?: { avatarColor?: string; avatarShape?: string }): AvatarConfig {
  // Donna exige escolha explícita. Personas só casam com o nome do bot.
  const roles = ROLES.filter((r) => r.id !== 'donna')
  const nameText = folded(name)
  const descriptionText = folded(description)
  const exact = roles.filter((r) => aliases(r, true).includes(nameText))
  const byName = roles.filter((r) => aliases(r, true).some((s) => contains(nameText, s)))
  const candidates = exact.length ? exact : byName.length ? byName : roles.filter((r) => aliases(r, false).some((s) => contains(descriptionText, s)))
  return candidates.length === 1 ? configFromRole(candidates[0]!) : legacyAvatar(legacy?.avatarShape, legacy?.avatarColor)
}

export function legacyAvatar(shape = '', color = ''): AvatarConfig {
  return { ...DEFAULT_CONFIG,
    shape: Object.hasOwn(data.legacyShapes, shape) ? data.legacyShapes[shape as keyof typeof data.legacyShapes] as AvatarConfig['shape'] : DEFAULT_CONFIG.shape,
    color: Object.hasOwn(data.legacyColors, color) ? data.legacyColors[color as keyof typeof data.legacyColors].avatarHex : DEFAULT_CONFIG.color,
  }
}

/** A escolha inicial varia sem copiar a aparência da principal. Os 30 presets seguem disponíveis no editor. */
export function randomAvatar(): AvatarConfig {
  const roles = ROLES.filter((r) => r.id !== 'donna')
  return configFromRole(roles[Math.floor(Math.random() * roles.length)]!)
}

export const botAccent = (bot: { avatarConfig?: unknown; mexeriquinha?: unknown; name?: string; description?: string }): string =>
  normalizedAvatar(avatarDoBot(bot), bot.name, bot.description).color

export const AVATAR_HUB_REQUIRED = traduzir('pt-BR', 'marca.avatar.hubAntigo')
export function avatarHubNotice(hello: { avatarEditing?: boolean } | null | undefined): string | null {
  if (hello == null) return 'Conectando ao Hub…'
  return hello.avatarEditing ? null : AVATAR_HUB_REQUIRED
}

export function requireAvatarHub(hello: { avatarEditing?: boolean } | null | undefined) {
  const notice = avatarHubNotice(hello)
  if (notice) throw new Error(notice)
}

/** Confere a resposta crua antes de o fallback de compatibilidade esconder um Hub sem suporte. */
export function confirmAvatar(value: unknown, expected: BotAvatarConfig) {
  if (!equivalenciaAvatar(value, expected)) {
    throw new Error(`O Hub não confirmou o avatar. ${AVATAR_HUB_REQUIRED}`)
  }
}

export function roleSuggestion(name: string, previous: string, next: string): string {
  return !name.trim() || name === previous ? next : name
}

/** Também protege estilos contra dados de cache/Hub antigo inválidos. O Hub é a autoridade. */
export function validAvatar(value: unknown): value is AvatarConfig {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const object = value as Record<string, unknown>
  const properties = schema.properties
  if (Object.keys(object).length !== Object.keys(properties).length) return false
  return Object.entries(properties).every(([key, rule]) => {
    const field = object[key]
    return key === 'color'
      ? typeof field === 'string' && /^#[0-9A-Fa-f]{6}$/.test(field) && field.length === 7
      : 'enum' in rule && (rule.enum as readonly unknown[]).includes(field)
  })
}

export const normalizedAvatar = (value: unknown, name = '', description = ''): AvatarConfig =>
  validAvatar(value) ? value : validMexeriquinha(value) ? projetarAvatarLegado(value) : configForRole(name, description)

export function avatarRole(config: AvatarConfig): string {
  return ROLES.find((r) => Object.entries(configFromRole(r)).every(([key, value]) => config[key as keyof AvatarConfig] === value))?.role ?? 'Bot personalizado'
}
