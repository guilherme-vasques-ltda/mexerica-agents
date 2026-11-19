import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
// SPDX-License-Identifier: Apache-2.0
// Contrato do fork; schema e dados próprios são incorporados também pelo Hub Rust.
import schema from './fork-mexeriquinha.schema.json' with { type: 'json' }
import data from './fork-mexeriquinha-data.json' with { type: 'json' }
import { configForRole, DEFAULT_CONFIG, validAvatar, type AvatarConfig } from './fork-avatar-config.ts'

export interface AparenciaMexeriquinha {
  cor: 'laranja' | 'lima' | 'limao' | 'toranja' | 'roxo' | 'celeste'
  olhos: 'ponto' | 'sonolentos' | 'brilhantes' | 'oculos-redondos' | 'oculos-escuros' | 'piscando' | 'oculos-verdes'
  roupa: 'nenhuma' | 'blazer' | 'colete' | 'moletom' | 'avental' | 'jaleco' | 'polo'
  acessorio: 'nenhum' | 'coroa' | 'fone' | 'capacete' | 'boina' | 'bone' | 'flor' | 'cartola' | 'capuz'
  objeto: 'nenhum' | 'gravata' | 'moeda' | 'chave' | 'megafone' | 'notebook' | 'lupa' | 'pincel' | 'caixa' | 'carrinho' | 'coracao' | 'ferramenta'
}
export type ConjuntoCatalogoId = string
export type MexeriquinhaConfig =
  | { v: 2; estilo: 'simples'; modeloId?: string; aparencia: AparenciaMexeriquinha }
  | { v: 2; estilo: '3d'; modeloId?: string; conjuntoId: ConjuntoCatalogoId }
  | { v: 3; estilo: '3d'; assetId: string; fallbackConjuntoId: ConjuntoCatalogoId }
export type BotAvatarConfig = AvatarConfig | MexeriquinhaConfig

// Subconjunto fechado de JSON Schema; metadados só na raiz. Os dois discriminantes
// fixos são disjuntos, portanto `some` equivale a oneOf. Keyword nova falha fechada.
const isObject = (value: unknown): value is Record<string, unknown> => Boolean(value) && typeof value === 'object' && !Array.isArray(value)
function supportedRule(value: unknown): boolean {
  if (!isObject(value)) return false
  const keys = Object.keys(value)
  if (keys.length === 2 && value.type === 'string' && value.format === 'assetId') return true
  if (Object.hasOwn(value, 'const')) return keys.length === 1 && (value.const === null || ['string', 'number', 'boolean'].includes(typeof value.const))
  if (Array.isArray(value.enum)) return keys.length === 1 && value.enum.length > 0 && value.enum.every((item) => typeof item === 'string') && new Set(value.enum).size === value.enum.length
  if (keys.length !== 4 || value.type !== 'object' || value.additionalProperties !== false || !isObject(value.properties) || !Array.isArray(value.required)) return false
  return Object.keys(value.properties).length > 0 && new Set(value.required).size === value.required.length && value.required.every((key) => typeof key === 'string' && Object.hasOwn(value.properties as object, key)) && Object.values(value.properties).every(supportedRule)
}
export function schemaMexeriquinhaSuportado(value: unknown): boolean {
  if (!isObject(value) || Object.entries(value).some(([key, item]) => key !== 'oneOf' && (!['$schema', '$id', 'title', 'description'].includes(key) || typeof item !== 'string')) || !Array.isArray(value.oneOf) || value.oneOf.length !== 3) return false
  const styles = new Set<string>()
  for (const branch of value.oneOf) {
    if (!supportedRule(branch) || !isObject(branch) || !isObject(branch.properties) || !Array.isArray(branch.required) || !branch.required.includes('v') || !branch.required.includes('estilo')) return false
    const version = branch.properties.v, style = branch.properties.estilo
    if (!isObject(version) || ![2, 3].includes(version.const as number) || !isObject(style) || typeof style.const !== 'string') return false
    styles.add(`${version.const}:${style.const}`)
  }
  return styles.has('2:simples') && styles.has('2:3d') && styles.has('3:3d')
}
const SCHEMA_SUPORTADO = schemaMexeriquinhaSuportado(schema)

type Rule = { const?: unknown; enum?: unknown[]; type?: string; format?: string; required?: string[]; properties?: Record<string, Rule | undefined> }
function matches(value: unknown, rule: Rule): boolean {
  if (rule.format === 'assetId') return assetIdValido(value)
  if ('const' in rule) return value === rule.const
  if (rule.enum) return rule.enum.includes(value)
  if (rule.type !== 'object' || !value || typeof value !== 'object' || Array.isArray(value)) return false
  const object = value as Record<string, unknown>
  return rule.required!.every((key) => Object.hasOwn(object, key)) && Object.keys(object).every((key) =>
    Object.hasOwn(rule.properties!, key) && matches(object[key], rule.properties![key]!))
}
function withinLimit(value: unknown): boolean {
  try {
    let bytes = 0
    for (const c of JSON.stringify(value)) { const n = c.codePointAt(0)!; bytes += n > 0xffff ? 4 : n > 0x7ff ? 3 : n > 0x7f ? 2 : 1 }
    return bytes <= 1024
  } catch { return false }
}
export function validMexeriquinha(value: unknown): value is MexeriquinhaConfig {
  return SCHEMA_SUPORTADO && withinLimit(value) && schema.oneOf.some((rule) => matches(value, rule))
}
export function validBotAvatar(value: unknown): value is BotAvatarConfig {
  return withinLimit(value) && (validAvatar(value) || validMexeriquinha(value))
}
const rgb = (hex: string): number[] => [1, 3, 5].map((n) => Number.parseInt(hex.slice(n, n + 2), 16))
function corProxima(hex: string): AparenciaMexeriquinha['cor'] {
  const target = rgb(hex)
  const distancia = (v: string) => rgb(v).reduce((n, c, i) => n + (c - target[i]!) ** 2, 0)
  // Mesmo desempate ordinal de IDs do Hub Rust, sem depender do locale ou da ordem do JSON.
  return Object.entries(data.cores).sort(([idA, a], [idB, b]) =>
    distancia(a) - distancia(b) || (idA < idB ? -1 : idA > idB ? 1 : 0))[0]![0] as AparenciaMexeriquinha['cor']
}
/** Aproxima a aparência legada, sem inferir instruções, papel ou autoridade. */
export function normalizarAvatarDoBot(value: unknown, name = '', description = ''): MexeriquinhaConfig {
  if (validMexeriquinha(value)) return structuredClone(value)
  const old = validAvatar(value) ? value : configForRole(name, description)
  const aparencia = { ...data.padrao, cor: corProxima(old.color) } as AparenciaMexeriquinha
  const eyes = data.olhosLegados as Record<string, AparenciaMexeriquinha['olhos']>
  const accessories = data.acessoriosLegados as Record<string, AparenciaMexeriquinha['acessorio']>
  const badges = data.badgesLegados as Record<string, Partial<AparenciaMexeriquinha>>
  aparencia.olhos = old.face === 'glasses' ? 'oculos-redondos' : old.face === 'shades' ? 'oculos-escuros' : eyes[old.eye] ?? 'ponto'
  aparencia.acessorio = accessories[old.head] ?? 'nenhum'
  if (old.neck === 'tie' || old.neck === 'bowtie') { aparencia.roupa = 'blazer'; aparencia.objeto = 'gravata' }
  Object.assign(aparencia, badges[old.badge] ?? {})
  return { v: 2, estilo: 'simples', aparencia }
}
/** uuid v4 canônico: o formato único autorizado para conjuntos gerados pelo Hub. */
export function assetIdValido(value: unknown): value is string {
  return typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value)
}

/** O id que resolve os quadros: catálogo em v2, assetId gerado em v3. */
export function conjuntoDoAvatar(value: MexeriquinhaConfig): string | null {
  if (value.estilo !== '3d') return null
  return value.v === 3 ? value.assetId : value.conjuntoId
}

/** Projeção para consumidores da biblioteca pública antiga, cujo schema não muda. */
export function projetarAvatarLegado(value: MexeriquinhaConfig): AvatarConfig {
  const appearance = value.estilo === 'simples' ? value.aparencia : (data.conjuntos as Record<string, AparenciaMexeriquinha>)[value.v === 3 ? value.fallbackConjuntoId : value.conjuntoId]!
  return { ...DEFAULT_CONFIG, shape: 'oval', color: data.cores[appearance.cor],
    eye: appearance.olhos === 'sonolentos' ? 'bar' : appearance.olhos === 'brilhantes' ? 'round' : appearance.olhos === 'piscando' ? 'cat' : 'auto',
    face: appearance.olhos === 'oculos-redondos' ? 'glasses' : appearance.olhos === 'oculos-escuros' || appearance.olhos === 'oculos-verdes' ? 'shades' : 'none' }
}
/** Igualdade estrutural de valores validados, independente da ordem das chaves. */
export function equivalenciaAvatar(a: unknown, b: unknown): boolean {
  if (!validBotAvatar(a) || !validBotAvatar(b)) return false
  const canonical = (v: unknown): string => v && typeof v === 'object'
    ? `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonical((v as Record<string, unknown>)[k])}`).join(',')}}`
    : JSON.stringify(v)
  return canonical(a) === canonical(b)
}
export function avatarMexeriquinhaHubNotice(hello: { avatarConfigVersion?: number; avatarEditing?: boolean } | null | undefined): string | null {
  if (hello == null) return traduzir('pt-BR', 'ajustes.estado.conectando')
  return typeof hello.avatarConfigVersion === 'number' && hello.avatarConfigVersion >= 2
    ? null : traduzir('pt-BR', 'marca.avatar.hubAntigo')
}
export function requireAvatarMexeriquinhaHub(hello: { avatarConfigVersion?: number; avatarEditing?: boolean } | null | undefined): void {
  const notice = avatarMexeriquinhaHubNotice(hello)
  if (notice) throw new Error(notice)
}

/** Fronteira pública: avatarConfig continua no schema legado; v2 usa chave própria do fork. */
export function camposAvatarDoBot(value: BotAvatarConfig): { avatarConfig: AvatarConfig; mexeriquinha?: MexeriquinhaConfig } {
  if (validMexeriquinha(value)) return { avatarConfig: projetarAvatarLegado(value), mexeriquinha: structuredClone(value) }
  if (!validAvatar(value)) throw new Error('invalid_avatar')
  return { avatarConfig: structuredClone(value) }
}
/** Uma chave nova só prevalece se sua projeção corresponde ao avatar público recebido. */
export function avatarDoBot(bot: { avatarConfig?: unknown; mexeriquinha?: unknown } | null | undefined): BotAvatarConfig | undefined {
  if (!bot) return undefined
  if (validMexeriquinha(bot.mexeriquinha) && (!validAvatar(bot.avatarConfig) || equivalenciaAvatar(projetarAvatarLegado(bot.mexeriquinha), bot.avatarConfig))) return bot.mexeriquinha
  return validAvatar(bot.avatarConfig) ? bot.avatarConfig : undefined
}
