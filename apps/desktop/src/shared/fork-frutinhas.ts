// D478 (dono, 07/11/2026): as frutinhas do dono (brand/assets/personagens/, #550) no lugar do rosto de robô.
// Regra só no app: a frutinha sai do selo (`badge`) do AvatarConfig salvo; nada é gravado nem migrado e o Hub não muda
// (ele valida o avatarConfig pelo schema da biblioteca, que continua igual). Docs: docs/reference/redesenho/components/BotFace.
import { configFromRole, ROLES, type AvatarConfig } from './fork-avatar-config.ts'

/** As 10 frutinhas, na ordem da grade do editor. */
export const FRUTINHAS = ['ceo', 'assistente', 'financeiro', 'marketing', 'developer', 'pesquisa', 'vendas', 'compras', 'rh', 'suporte'] as const
export type Frutinha = (typeof FRUTINHAS)[number]
/** O que o rosto desenha no lugar do robô: uma frutinha ou a mascote do pacote (#542). */
export type ArteDoRosto = Frutinha | 'mexerica'

/**
 * D486 (dono): mascote. O selo `sparkle` é o do preset do principal (o Berga), que usa a mascote Mexerica do pacote
 * (#542) como rosto. `'atual'` voltaria ao robô de antes; trocar o rosto do Berga é mudar só esta constante.
 */
export type RostoPrincipal = 'atual' | 'mascote'
export const ROSTO_PRINCIPAL: RostoPrincipal = 'mascote'

/** Selo de função → frutinha (os 30 presets da biblioteca; cada preset tem um selo próprio). */
export const FRUTA_DO_SELO: Readonly<Partial<Record<AvatarConfig['badge'], Frutinha>>> = {
  briefcase: 'ceo', kanban: 'ceo',
  calendar: 'assistente', notes: 'assistente', globe: 'assistente', translate: 'assistente',
  search: 'pesquisa',
  code: 'developer', bug: 'developer',
  chart: 'vendas', phone: 'vendas', bag: 'vendas', box: 'vendas',
  cart: 'compras', ship: 'compras', warehouse: 'compras',
  lifebuoy: 'suporte',
  megaphone: 'marketing', target: 'marketing', chat: 'marketing', pen: 'marketing', seo: 'marketing', video: 'marketing', palette: 'marketing',
  coin: 'financeiro', calc: 'financeiro', ledger: 'financeiro',
  people: 'rh', heart: 'rh',
}

/**
 * A arte do rosto para esta aparência, ou null para o robô de sempre. Lista fechada: só um selo que é, exatamente, uma
 * chave própria de FRUTA_DO_SELO (os selos de função dos 30 presets) vira frutinha; qualquer outro valor (sem função,
 * desconhecido, adulterado ou chave do protótipo) cai no robô. Robô editado à mão com selo de função vira a frutinha.
 */
export function arteDoAvatar(config: Pick<AvatarConfig, 'badge'>, rostoPrincipal: RostoPrincipal = ROSTO_PRINCIPAL): ArteDoRosto | null {
  if (config.badge === 'sparkle') return rostoPrincipal === 'mascote' ? 'mexerica' : null
  return Object.hasOwn(FRUTA_DO_SELO, config.badge) ? FRUTA_DO_SELO[config.badge] ?? null : null
}

/** O preset que cada frutinha aplica no editor ("Frutinha" escolhida → aparência dessa função). */
const PRESET_DA_FRUTINHA: Record<Frutinha, string> = {
  ceo: 'chief', assistente: 'calendar', financeiro: 'financeiro', marketing: 'marketing', developer: 'dev',
  pesquisa: 'research', vendas: 'vendas', compras: 'compras', rh: 'rh', suporte: 'suporte',
}

export function configDaFrutinha(frutinha: Frutinha): AvatarConfig {
  return configFromRole(ROLES.find((r) => r.id === PRESET_DA_FRUTINHA[frutinha])!)
}

/** A aparência do preset do principal (selo `sparkle`): o rosto do Berga (D486). */
export const configDoPrincipal = (): AvatarConfig => configFromRole(ROLES.find((r) => r.id === 'donna')!)
