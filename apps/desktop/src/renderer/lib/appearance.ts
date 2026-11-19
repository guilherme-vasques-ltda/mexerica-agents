import type { SpaceAppearance } from '../../shared/fork-spaces'
import { useSyncExternalStore } from 'react'
import { pref } from './prefs'
import { toquesMexericaPref } from './toques-mexerica'
import { TOQUES_HOVER_FILL, toquesHoverInk } from './toques-hover'
import { channelBounds, controlGlassAlpha, effectiveGlass, glassAlpha, layerGround, legibleOn, over, readableForeground, safeFloor, saturatedBounds, veiledBounds, worstOf, type Bounds, type GlassLayer, type RGB } from './legibility'
import { Intents } from './intents'
import { areaChoice, conversationPlate, detailsLayer, isAreaValues, isTransparencyMode, type AreaValues, type TransparencyArea, type TransparencyMode } from './transparency-areas'
import { CODE_WALLPAPERS, type CodeWallpaperId } from './fork-wallpapers-code'
import { ptBR } from './fork-i18n/pt-BR.ts'
import type { ChaveTexto } from './fork-i18n/index.ts'
import { DEFAULT_APP_FONT, DEFAULT_TAG_COLORS, fontStack, isAppFont, readTagColors, tagColorVars, type AppFont } from './appearance-choices'
import serra from '../assets/wallpapers/serra.webp'
import serraEscuro from '../assets/wallpapers/serra-escuro.webp'
import duna from '../assets/wallpapers/duna.webp'
import dunaEscuro from '../assets/wallpapers/duna-escuro.webp'
import mare from '../assets/wallpapers/mare.webp'
import mareEscuro from '../assets/wallpapers/mare-escuro.webp'
import noite from '../assets/wallpapers/noite.webp'
import cidade from '../assets/wallpapers/cidade.webp'
import cidadeEscuro from '../assets/wallpapers/cidade-escuro.webp'
import risca from '../assets/wallpapers/risca.webp'
import riscaEscuro from '../assets/wallpapers/risca-escuro.webp'
import mogno from '../assets/wallpapers/mogno.webp'
import mognoEscuro from '../assets/wallpapers/mogno-escuro.webp'
import cafeteria from '../assets/wallpapers/cafeteria.webp'
import cafeteriaEscuro from '../assets/wallpapers/cafeteria-escuro.webp'
import cafe from '../assets/wallpapers/cafe.webp'
import cafeEscuro from '../assets/wallpapers/cafe-escuro.webp'
import campo from '../assets/wallpapers/campo.webp'
import campoEscuro from '../assets/wallpapers/campo-escuro.webp'
import lago from '../assets/wallpapers/lago.webp'
import lagoEscuro from '../assets/wallpapers/lago-escuro.webp'
import skyline from '../assets/wallpapers/skyline.webp'
import skylineEscuro from '../assets/wallpapers/skyline-escuro.webp'
import escritorio from '../assets/wallpapers/escritorio.webp'
import escritorioEscuro from '../assets/wallpapers/escritorio-escuro.webp'
import ondas from '../assets/wallpapers/ondas.webp'
import ondasEscuro from '../assets/wallpapers/ondas-escuro.webp'
import circulos from '../assets/wallpapers/circulos.webp'
import circulosEscuro from '../assets/wallpapers/circulos-escuro.webp'
import blocos from '../assets/wallpapers/blocos.webp'
import blocosEscuro from '../assets/wallpapers/blocos-escuro.webp'
import jardim from '../assets/wallpapers/jardim.webp'
import jardimEscuro from '../assets/wallpapers/jardim-escuro.webp'
// PR g (07/11): fundos da Mexerica (#522, brand/assets/fundos-chat), em WebP.
import auroraSuave from '../assets/wallpapers/aurora-suave.webp'
import auroraSuaveEscuro from '../assets/wallpapers/aurora-suave-escuro.webp'
import janelaDaCidade from '../assets/wallpapers/janela-da-cidade.webp'
import janelaDaCidadeEscuro from '../assets/wallpapers/janela-da-cidade-escuro.webp'
import jardimCitrico from '../assets/wallpapers/jardim-citrico.webp'
import jardimCitricoEscuro from '../assets/wallpapers/jardim-citrico-escuro.webp'
import ondasPastel from '../assets/wallpapers/ondas-pastel.webp'
import ondasPastelEscuro from '../assets/wallpapers/ondas-pastel-escuro.webp'
import montanhasSuaves from '../assets/wallpapers/montanhas-suaves.webp'
import montanhasSuavesEscuro from '../assets/wallpapers/montanhas-suaves-escuro.webp'
import linhoFino from '../assets/wallpapers/linho-fino.webp'
import linhoFinoEscuro from '../assets/wallpapers/linho-fino-escuro.webp'
import folhasBotanicas from '../assets/wallpapers/folhas-botanicas.webp'
import folhasBotanicasEscuro from '../assets/wallpapers/folhas-botanicas-escuro.webp'
import nuvensSuaves from '../assets/wallpapers/nuvens-suaves.webp'
import nuvensSuavesEscuro from '../assets/wallpapers/nuvens-suaves-escuro.webp'
import cantoAconchegante from '../assets/wallpapers/canto-aconchegante.webp'
import cantoAconcheganteEscuro from '../assets/wallpapers/canto-aconchegante-escuro.webp'
import vidroCitrico from '../assets/wallpapers/vidro-citrico.webp'
import vidroCitricoEscuro from '../assets/wallpapers/vidro-citrico-escuro.webp'

// MX-79: miniaturas de 256 px da grade de Aparência (tools/otimizar-imagens.py); a tela não decodifica o fundo cheio.
import serraThumb from '../assets/wallpapers/thumbs/serra.webp'
import serraEscuroThumb from '../assets/wallpapers/thumbs/serra-escuro.webp'
import dunaThumb from '../assets/wallpapers/thumbs/duna.webp'
import dunaEscuroThumb from '../assets/wallpapers/thumbs/duna-escuro.webp'
import mareThumb from '../assets/wallpapers/thumbs/mare.webp'
import mareEscuroThumb from '../assets/wallpapers/thumbs/mare-escuro.webp'
import noiteThumb from '../assets/wallpapers/thumbs/noite.webp'
import cidadeThumb from '../assets/wallpapers/thumbs/cidade.webp'
import cidadeEscuroThumb from '../assets/wallpapers/thumbs/cidade-escuro.webp'
import riscaThumb from '../assets/wallpapers/thumbs/risca.webp'
import riscaEscuroThumb from '../assets/wallpapers/thumbs/risca-escuro.webp'
import mognoThumb from '../assets/wallpapers/thumbs/mogno.webp'
import mognoEscuroThumb from '../assets/wallpapers/thumbs/mogno-escuro.webp'
import cafeteriaThumb from '../assets/wallpapers/thumbs/cafeteria.webp'
import cafeteriaEscuroThumb from '../assets/wallpapers/thumbs/cafeteria-escuro.webp'
import cafeThumb from '../assets/wallpapers/thumbs/cafe.webp'
import cafeEscuroThumb from '../assets/wallpapers/thumbs/cafe-escuro.webp'
import campoThumb from '../assets/wallpapers/thumbs/campo.webp'
import campoEscuroThumb from '../assets/wallpapers/thumbs/campo-escuro.webp'
import lagoThumb from '../assets/wallpapers/thumbs/lago.webp'
import lagoEscuroThumb from '../assets/wallpapers/thumbs/lago-escuro.webp'
import skylineThumb from '../assets/wallpapers/thumbs/skyline.webp'
import skylineEscuroThumb from '../assets/wallpapers/thumbs/skyline-escuro.webp'
import escritorioThumb from '../assets/wallpapers/thumbs/escritorio.webp'
import escritorioEscuroThumb from '../assets/wallpapers/thumbs/escritorio-escuro.webp'
import ondasThumb from '../assets/wallpapers/thumbs/ondas.webp'
import ondasEscuroThumb from '../assets/wallpapers/thumbs/ondas-escuro.webp'
import circulosThumb from '../assets/wallpapers/thumbs/circulos.webp'
import circulosEscuroThumb from '../assets/wallpapers/thumbs/circulos-escuro.webp'
import blocosThumb from '../assets/wallpapers/thumbs/blocos.webp'
import blocosEscuroThumb from '../assets/wallpapers/thumbs/blocos-escuro.webp'
import jardimThumb from '../assets/wallpapers/thumbs/jardim.webp'
import jardimEscuroThumb from '../assets/wallpapers/thumbs/jardim-escuro.webp'
import auroraSuaveThumb from '../assets/wallpapers/thumbs/aurora-suave.webp'
import auroraSuaveEscuroThumb from '../assets/wallpapers/thumbs/aurora-suave-escuro.webp'
import janelaDaCidadeThumb from '../assets/wallpapers/thumbs/janela-da-cidade.webp'
import janelaDaCidadeEscuroThumb from '../assets/wallpapers/thumbs/janela-da-cidade-escuro.webp'
import jardimCitricoThumb from '../assets/wallpapers/thumbs/jardim-citrico.webp'
import jardimCitricoEscuroThumb from '../assets/wallpapers/thumbs/jardim-citrico-escuro.webp'
import ondasPastelThumb from '../assets/wallpapers/thumbs/ondas-pastel.webp'
import ondasPastelEscuroThumb from '../assets/wallpapers/thumbs/ondas-pastel-escuro.webp'
import montanhasSuavesThumb from '../assets/wallpapers/thumbs/montanhas-suaves.webp'
import montanhasSuavesEscuroThumb from '../assets/wallpapers/thumbs/montanhas-suaves-escuro.webp'
import linhoFinoThumb from '../assets/wallpapers/thumbs/linho-fino.webp'
import linhoFinoEscuroThumb from '../assets/wallpapers/thumbs/linho-fino-escuro.webp'
import folhasBotanicasThumb from '../assets/wallpapers/thumbs/folhas-botanicas.webp'
import folhasBotanicasEscuroThumb from '../assets/wallpapers/thumbs/folhas-botanicas-escuro.webp'
import nuvensSuavesThumb from '../assets/wallpapers/thumbs/nuvens-suaves.webp'
import nuvensSuavesEscuroThumb from '../assets/wallpapers/thumbs/nuvens-suaves-escuro.webp'
import cantoAconcheganteThumb from '../assets/wallpapers/thumbs/canto-aconchegante.webp'
import cantoAconcheganteEscuroThumb from '../assets/wallpapers/thumbs/canto-aconchegante-escuro.webp'
import vidroCitricoThumb from '../assets/wallpapers/thumbs/vidro-citrico.webp'
import vidroCitricoEscuroThumb from '../assets/wallpapers/thumbs/vidro-citrico-escuro.webp'

// D165, fase A: the Aparência preferences (docs/reference/redesenho/personalizacao.md) and the variables
// they write on <html>: the theme, the accent, the wallpaper with its blur and veil, and the glass at
// its effective opacity. Local to this Mac (no Hub field). There are no spaces yet, so the wallpaper
// preferences are the app's: they become the Pessoal space's when spaces arrive.

export type ThemeChoice = 'system' | 'light' | 'dark'
export type AccentChoice = 'mexerica' | 'folha' | 'verde' | 'violeta' | 'azul' | 'jade' | 'ambar' | 'rosa' | 'grafite' | 'laranja-claro' | 'verde-claro'
/** D529: Mexerica só para quem nunca escolheu. Uma escolha salva (inclusive Verde) continua valendo. */
export const DEFAULT_ACCENT: AccentChoice = 'mexerica'
/** Fundos da Mexerica (PR g): nomes no catálogo `aparencia.fundo.<id>`. */
export type MexericaWallpaperId = 'aurora-suave' | 'janela-da-cidade' | 'jardim-citrico' | 'ondas-pastel' | 'montanhas-suaves' | 'linho-fino' | 'folhas-botanicas' | 'nuvens-suaves' | 'canto-aconchegante' | 'vidro-citrico'
export type WallpaperId = 'none' | 'serra' | 'duna' | 'mare' | 'noite' | 'cidade' | 'risca' | 'mogno' | 'cafeteria' | 'cafe' | 'campo' | 'lago' | 'skyline' | 'escritorio' | 'ondas' | 'circulos' | 'blocos' | 'jardim' | MexericaWallpaperId | CodeWallpaperId | 'custom'

/** D454: os fundos da casa com nome no catálogo (`ajustes.fundo.<id>`). */
type HouseWallpaperId = 'serra' | 'duna' | 'mare' | 'noite' | 'cidade' | 'risca' | 'mogno' | 'cafeteria' | 'cafe' | 'campo' | 'lago' | 'skyline' | 'escritorio' | 'ondas' | 'circulos' | 'blocos' | 'jardim'

// MX-143 (decisão do dono 11/11): os fundos donna-* saíram do catálogo; instalação nova e preferência
// ausente/vazia usam Aurora suave, o primeiro da galeria.
export const DEFAULT_WALLPAPER: WallpaperId = 'aurora-suave'

// D454 (fatia 4): cada escolha leva a chave do catálogo (`labelKey`); `label` é o texto pt-BR do catálogo, para quem
// ainda lê o rótulo direto. A tela de Aparência mostra `t(labelKey)`.
export const THEMES: { id: ThemeChoice; label: string; labelKey: ChaveTexto }[] = [
  { id: 'system', label: ptBR['ajustes.tema.sistema'], labelKey: 'ajustes.tema.sistema' },
  { id: 'light', label: ptBR['ajustes.tema.claro'], labelKey: 'ajustes.tema.claro' },
  { id: 'dark', label: ptBR['ajustes.tema.escuro'], labelKey: 'ajustes.tema.escuro' },
]

export const ACCENTS: { id: AccentChoice; label: string; labelKey: ChaveTexto }[] = [
  { id: 'mexerica', label: ptBR['ajustes.cor.mexerica'], labelKey: 'ajustes.cor.mexerica' },
  { id: 'folha', label: ptBR['ajustes.cor.folha'], labelKey: 'ajustes.cor.folha' },
  { id: 'verde', label: ptBR['ajustes.cor.verde'], labelKey: 'ajustes.cor.verde' },
  { id: 'violeta', label: ptBR['ajustes.cor.violeta'], labelKey: 'ajustes.cor.violeta' },
  { id: 'azul', label: ptBR['ajustes.cor.azul'], labelKey: 'ajustes.cor.azul' },
  { id: 'jade', label: ptBR['ajustes.cor.jade'], labelKey: 'ajustes.cor.jade' },
  { id: 'ambar', label: ptBR['ajustes.cor.ambar'], labelKey: 'ajustes.cor.ambar' },
  { id: 'rosa', label: ptBR['ajustes.cor.rosa'], labelKey: 'ajustes.cor.rosa' },
  { id: 'grafite', label: ptBR['ajustes.cor.grafite'], labelKey: 'ajustes.cor.grafite' },
  { id: 'laranja-claro', label: ptBR['ajustes.cor.laranjaClaro'], labelKey: 'ajustes.cor.laranjaClaro' },
  { id: 'verde-claro', label: ptBR['ajustes.cor.verdeClaro'], labelKey: 'ajustes.cor.verdeClaro' },
]

interface HouseWallpaper {
  id: Exclude<WallpaperId, 'none' | 'custom'>
  label: string
  labelKey?: `aparencia.fundo.${CodeWallpaperId | MexericaWallpaperId}` | `ajustes.fundo.${HouseWallpaperId}`
  light: string
  dark: string
  /** MX-79: miniatura WebP de 256 px para a grade de Aparência; fundos em código (SVG) não têm e usam a imagem cheia. */
  thumbLight?: string
  thumbDark?: string
  /**
   * A safe upper bound of the floor of the full-resolution picture with no veil, both themes (taken when the
   * floor still held `ink-2`, so it is higher than `floorFor` now gives), used only until the picture is read back
   * or when the canvas can't read it (D165 revisão A, F2).
   * Fundos abstratos: medida conservadora dos JPGs finais, com ink-2 e a tinta composta do Berga;
   * extremos RGB, hashes e receita em apps/desktop/design/fundos/medidas.json e README.md.
   */
  floor: { light: number; dark: number }
}

/**
 * PR g: pisos conservadores sem véu dos fundos da Mexerica (WebP finais medidos no Chromium; receita e números em
 * apps/desktop/design/fundos-mexerica/medidas.json e README.md). Usados até a leitura da imagem, como os dos outros fundos.
 */
const MEXERICA_FLOORS: Record<MexericaWallpaperId, { light: number; dark: number }> = {
  'aurora-suave': { light: 0.96, dark: 0.95 },
  'janela-da-cidade': { light: 0.97, dark: 0.96 },
  'jardim-citrico': { light: 0.98, dark: 0.96 },
  'ondas-pastel': { light: 0.95, dark: 0.95 },
  'montanhas-suaves': { light: 0.96, dark: 0.94 },
  'linho-fino': { light: 0.93, dark: 0.91 },
  'folhas-botanicas': { light: 0.99, dark: 0.96 },
  'nuvens-suaves': { light: 0.94, dark: 0.96 },
  'canto-aconchegante': { light: 0.98, dark: 0.97 },
  'vidro-citrico': { light: 0.95, dark: 0.93 },
}

export const HOUSE_WALLPAPERS: HouseWallpaper[] = [
  // PR g: os fundos da Mexerica abrem a galeria; pisos medidos em apps/desktop/design/fundos-mexerica/medidas.json.
  { id: 'aurora-suave', label: ptBR['aparencia.fundo.aurora-suave'], labelKey: 'aparencia.fundo.aurora-suave', light: auroraSuave, dark: auroraSuaveEscuro, thumbLight: auroraSuaveThumb, thumbDark: auroraSuaveEscuroThumb, floor: MEXERICA_FLOORS['aurora-suave'] },
  { id: 'janela-da-cidade', label: ptBR['aparencia.fundo.janela-da-cidade'], labelKey: 'aparencia.fundo.janela-da-cidade', light: janelaDaCidade, dark: janelaDaCidadeEscuro, thumbLight: janelaDaCidadeThumb, thumbDark: janelaDaCidadeEscuroThumb, floor: MEXERICA_FLOORS['janela-da-cidade'] },
  { id: 'jardim-citrico', label: ptBR['aparencia.fundo.jardim-citrico'], labelKey: 'aparencia.fundo.jardim-citrico', light: jardimCitrico, dark: jardimCitricoEscuro, thumbLight: jardimCitricoThumb, thumbDark: jardimCitricoEscuroThumb, floor: MEXERICA_FLOORS['jardim-citrico'] },
  { id: 'ondas-pastel', label: ptBR['aparencia.fundo.ondas-pastel'], labelKey: 'aparencia.fundo.ondas-pastel', light: ondasPastel, dark: ondasPastelEscuro, thumbLight: ondasPastelThumb, thumbDark: ondasPastelEscuroThumb, floor: MEXERICA_FLOORS['ondas-pastel'] },
  { id: 'montanhas-suaves', label: ptBR['aparencia.fundo.montanhas-suaves'], labelKey: 'aparencia.fundo.montanhas-suaves', light: montanhasSuaves, dark: montanhasSuavesEscuro, thumbLight: montanhasSuavesThumb, thumbDark: montanhasSuavesEscuroThumb, floor: MEXERICA_FLOORS['montanhas-suaves'] },
  { id: 'linho-fino', label: ptBR['aparencia.fundo.linho-fino'], labelKey: 'aparencia.fundo.linho-fino', light: linhoFino, dark: linhoFinoEscuro, thumbLight: linhoFinoThumb, thumbDark: linhoFinoEscuroThumb, floor: MEXERICA_FLOORS['linho-fino'] },
  { id: 'folhas-botanicas', label: ptBR['aparencia.fundo.folhas-botanicas'], labelKey: 'aparencia.fundo.folhas-botanicas', light: folhasBotanicas, dark: folhasBotanicasEscuro, thumbLight: folhasBotanicasThumb, thumbDark: folhasBotanicasEscuroThumb, floor: MEXERICA_FLOORS['folhas-botanicas'] },
  { id: 'nuvens-suaves', label: ptBR['aparencia.fundo.nuvens-suaves'], labelKey: 'aparencia.fundo.nuvens-suaves', light: nuvensSuaves, dark: nuvensSuavesEscuro, thumbLight: nuvensSuavesThumb, thumbDark: nuvensSuavesEscuroThumb, floor: MEXERICA_FLOORS['nuvens-suaves'] },
  { id: 'canto-aconchegante', label: ptBR['aparencia.fundo.canto-aconchegante'], labelKey: 'aparencia.fundo.canto-aconchegante', light: cantoAconchegante, dark: cantoAconcheganteEscuro, thumbLight: cantoAconcheganteThumb, thumbDark: cantoAconcheganteEscuroThumb, floor: MEXERICA_FLOORS['canto-aconchegante'] },
  { id: 'vidro-citrico', label: ptBR['aparencia.fundo.vidro-citrico'], labelKey: 'aparencia.fundo.vidro-citrico', light: vidroCitrico, dark: vidroCitricoEscuro, thumbLight: vidroCitricoThumb, thumbDark: vidroCitricoEscuroThumb, floor: MEXERICA_FLOORS['vidro-citrico'] },
  // DON-909: fundos desenhados em código (lib/fork-wallpapers-code.ts); mesmo caminho dos JPGs.
  ...CODE_WALLPAPERS,
  { id: 'serra', label: ptBR['ajustes.fundo.serra'], labelKey: 'ajustes.fundo.serra', light: serra, dark: serraEscuro, thumbLight: serraThumb, thumbDark: serraEscuroThumb, floor: { light: 0.96, dark: 0.91 } },
  { id: 'duna', label: ptBR['ajustes.fundo.duna'], labelKey: 'ajustes.fundo.duna', light: duna, dark: dunaEscuro, thumbLight: dunaThumb, thumbDark: dunaEscuroThumb, floor: { light: 0.96, dark: 0.91 } },
  { id: 'mare', label: ptBR['ajustes.fundo.mare'], labelKey: 'ajustes.fundo.mare', light: mare, dark: mareEscuro, thumbLight: mareThumb, thumbDark: mareEscuroThumb, floor: { light: 0.96, dark: 0.91 } },
  { id: 'noite', label: ptBR['ajustes.fundo.noite'], labelKey: 'ajustes.fundo.noite', light: noite, dark: noite, thumbLight: noiteThumb, thumbDark: noiteThumb, floor: { light: 0.96, dark: 0.91 } },
  { id: 'cidade', label: ptBR['ajustes.fundo.cidade'], labelKey: 'ajustes.fundo.cidade', light: cidade, dark: cidadeEscuro, thumbLight: cidadeThumb, thumbDark: cidadeEscuroThumb, floor: { light: 0.96, dark: 0.91 } },
  { id: 'risca', label: ptBR['ajustes.fundo.risca'], labelKey: 'ajustes.fundo.risca', light: risca, dark: riscaEscuro, thumbLight: riscaThumb, thumbDark: riscaEscuroThumb, floor: { light: 0.88, dark: 0.79 } },
  { id: 'mogno', label: ptBR['ajustes.fundo.mogno'], labelKey: 'ajustes.fundo.mogno', light: mogno, dark: mognoEscuro, thumbLight: mognoThumb, thumbDark: mognoEscuroThumb, floor: { light: 0.94, dark: 0.76 } },
  { id: 'cafeteria', label: ptBR['ajustes.fundo.cafeteria'], labelKey: 'ajustes.fundo.cafeteria', light: cafeteria, dark: cafeteriaEscuro, thumbLight: cafeteriaThumb, thumbDark: cafeteriaEscuroThumb, floor: { light: 0.97, dark: 0.97 } },
  { id: 'cafe', label: ptBR['ajustes.fundo.cafe'], labelKey: 'ajustes.fundo.cafe', light: cafe, dark: cafeEscuro, thumbLight: cafeThumb, thumbDark: cafeEscuroThumb, floor: { light: 0.97, dark: 0.97 } },
  { id: 'campo', label: ptBR['ajustes.fundo.campo'], labelKey: 'ajustes.fundo.campo', light: campo, dark: campoEscuro, thumbLight: campoThumb, thumbDark: campoEscuroThumb, floor: { light: 0.95, dark: 0.97 } },
  { id: 'lago', label: ptBR['ajustes.fundo.lago'], labelKey: 'ajustes.fundo.lago', light: lago, dark: lagoEscuro, thumbLight: lagoThumb, thumbDark: lagoEscuroThumb, floor: { light: 0.95, dark: 0.97 } },
  { id: 'skyline', label: ptBR['ajustes.fundo.skyline'], labelKey: 'ajustes.fundo.skyline', light: skyline, dark: skylineEscuro, thumbLight: skylineThumb, thumbDark: skylineEscuroThumb, floor: { light: 0.96, dark: 0.97 } },
  { id: 'escritorio', label: ptBR['ajustes.fundo.escritorio'], labelKey: 'ajustes.fundo.escritorio', light: escritorio, dark: escritorioEscuro, thumbLight: escritorioThumb, thumbDark: escritorioEscuroThumb, floor: { light: 0.94, dark: 0.97 } },
  { id: 'ondas', label: ptBR['ajustes.fundo.ondas'], labelKey: 'ajustes.fundo.ondas', light: ondas, dark: ondasEscuro, thumbLight: ondasThumb, thumbDark: ondasEscuroThumb, floor: { light: 0.92, dark: 0.97 } },
  { id: 'circulos', label: ptBR['ajustes.fundo.circulos'], labelKey: 'ajustes.fundo.circulos', light: circulos, dark: circulosEscuro, thumbLight: circulosThumb, thumbDark: circulosEscuroThumb, floor: { light: 0.93, dark: 0.97 } },
  { id: 'blocos', label: ptBR['ajustes.fundo.blocos'], labelKey: 'ajustes.fundo.blocos', light: blocos, dark: blocosEscuro, thumbLight: blocosThumb, thumbDark: blocosEscuroThumb, floor: { light: 0.94, dark: 0.97 } },
  { id: 'jardim', label: ptBR['ajustes.fundo.jardim'], labelKey: 'ajustes.fundo.jardim', light: jardim, dark: jardimEscuro, thumbLight: jardimThumb, thumbDark: jardimEscuroThumb, floor: { light: 0.94, dark: 0.97 } },
]

/**
 * aparencia-01 (coordenação, D229/D244): Noite has only a night picture; under the light theme's glass it turned a
 * dull grey. It is offered in the dark theme only; chosen there, the light theme shows no wallpaper (the choice
 * is kept and comes back in the dark theme).
 */
const DARK_ONLY: ReadonlySet<WallpaperId> = new Set<WallpaperId>(['noite'])
/** Whether a wallpaper is offered (and shown) in this theme. */
export const offeredIn = (id: WallpaperId, dark: boolean) => dark || !DARK_ONLY.has(id)

/** Desfoque's ceiling (`blur-max`) and Suavizar's (`opacity-veil-max`). */
export const BLUR_MAX = 40
export const VEIL_MAX = 0.6
/**
 * Suavizar's default per theme (the token `wallpaper-veil`). aparencia-01: the dark one went from 0.32 to
 * 0.12, since the night pictures are dark already and a black veil on them hid the wallpaper.
 */
export const VEIL_DEFAULT = { light: 0.18, dark: 0.12 }
/** A picture of their own is reduced to this on its longest side before it's kept. */
const CUSTOM_MAX_SIDE = 2880
/** MX-79: a cópia da grade de Aparência, no maior lado (o cartão é 64×42 px). */
const CUSTOM_THUMB_SIDE = 384
/** The pictures the picker offers: what the app can open. HEIC (iPhone and Mac photos) isn't (aparencia-01). */
export const CUSTOM_ACCEPT = 'image/png,image/jpeg,image/webp'
/** The panel's sentences when a picture can't be used: fixed, never the browser's own (English) text. */
export const CUSTOM_ERRORS = {
  heic: ptBR['ajustes.fundo.erroHeic'],
  unreadable: ptBR['ajustes.fundo.erroIlegivel'],
} as const
/** D454: a chave de cada frase acima, para a tela mostrar no idioma escolhido. */
export const CUSTOM_ERROR_KEYS = { heic: 'ajustes.fundo.erroHeic', unreadable: 'ajustes.fundo.erroIlegivel' } as const
const isHeic = (file: File) => /^image\/hei[cf]/i.test(file.type) || /\.hei[cf]s?$/i.test(file.name)
/** PNG and WebP may have transparent parts: kept as WebP so they show over the theme's canvas, never black. */
const keepsAlpha = (file: File) => /^image\/(png|webp)$/i.test(file.type) || /\.(png|webp)$/i.test(file.name)

/** MX-143: IDs aceitos pela preferência; um fundo que saiu do catálogo (ex.: donna-*) cai no padrão, sem tela vazia. */
const WALLPAPER_CHOICE_IDS: ReadonlySet<string> = new Set([...HOUSE_WALLPAPERS.map((w) => w.id), 'none', 'custom'])

export const appearancePrefs = {
  mexericaTouches: toquesMexericaPref,
  theme: pref<ThemeChoice>('appearanceTheme', 'system'),
  accent: pref<AccentChoice>('appearanceAccent', DEFAULT_ACCENT, (value): value is AccentChoice => typeof value === 'string' && ACCENTS.some((a) => a.id === value)),
  /** Barra04: Ajustes → Aparência → Fonte (lib/appearance-choices.ts); padrão DM Sans. */
  font: pref<AppFont>('appearanceFont', DEFAULT_APP_FONT, isAppFont),
  /** Barra04: Ajustes → Barra lateral → Cores das etiquetas, por nível; padrão Grafite. Lido com `readTagColors`. */
  sidebarTagColors: pref<unknown>('sidebarTagColors', DEFAULT_TAG_COLORS),
  wallpaper: pref<WallpaperId>('appearanceWallpaper', DEFAULT_WALLPAPER, (value): value is WallpaperId => typeof value === 'string' && WALLPAPER_CHOICE_IDS.has(value)),
  /** px, 0…BLUR_MAX. */
  blur: pref('appearanceBlur', 0),
  /** Transparência, 0–100%: the general value (Geral), and the base of every area without its own. */
  transparency: pref('appearanceTransparency', 50),
  /** D169: Geral (one value on the four areas) or Por área (lib/transparency-areas.ts). */
  transparencyMode: pref<TransparencyMode>('appearanceTransparencyMode', 'general', isTransparencyMode),
  /** D169: the areas with a value of their own, 0–100%; a missing area follows the general value. */
  transparencyAreas: pref<AreaValues>('appearanceTransparencyAreas', {}, isAreaValues),
  /** Suavizar: the veil's alpha, 0…VEIL_MAX; null follows the theme's default. */
  veil: pref<number | null>('appearanceVeil', null),
}

// System preferences read the way lib/theme.ts reads them (one list read, another listened to).
function query(media: string) {
  const list = window.matchMedia(media)
  return { get: () => list.matches, list }
}
const systemDark = query('(prefers-color-scheme: dark)')
const reduceTransparency = query('(prefers-reduced-transparency: reduce)')
const moreContrast = query('(prefers-contrast: more)')

export const isSystemDark = systemDark.get

/** The theme in use: the choice, or the system's when the choice is Sistema. */
export function resolvedDark() {
  const theme = appearancePrefs.theme.get()
  return theme === 'system' ? systemDark.get() : theme === 'dark'
}

// --- The state the panel shows: the measured floor and the glass actually in use. ---

export interface AppearanceState {
  dark: boolean
  /** The wallpaper's floor for the current theme, blur and veil (0 with no wallpaper). */
  floor: number
  /** The glass opacity in use. */
  glass: number
  /** The glass the person picked, before the floor. */
  chosen: number
  /** True when the floor (or the system's contrast) holds the glass above the choice. */
  held: boolean
  /** Object URL of the person's own picture, once loaded. */
  customURL: string | null
  /** MX-79: object URL of its 384 px thumbnail for the Aparência grid; null for pictures stored before it existed. */
  customThumbURL: string | null
  /** The Mac asks for less transparency: every glass is opaque. */
  reducedTransparency: boolean
  /** The Mac asks for more contrast: the glass stays at HIGH_CONTRAST_FLOOR or above. */
  moreContrast: boolean
  /**
   * D169: each area's glass in use and as picked. The conversation's is its plate (opaque at 1, none at
   * 0), which has no floor: no text lies on it.
   */
  areas: Record<TransparencyArea, AreaGlass>
}

export interface AreaGlass {
  glass: number
  chosen: number
  held: boolean
}

const opaqueArea: AreaGlass = { glass: 1, chosen: 1, held: false }
let state: AppearanceState = {
  dark: false, floor: 0, glass: 1, chosen: 1, held: false, customURL: null, customThumbURL: null, reducedTransparency: false, moreContrast: false,
  areas: { sidebar: opaqueArea, details: opaqueArea, composer: opaqueArea, conversation: { glass: 0, chosen: 0, held: false } },
}
const listeners = new Set<() => void>()
function setState(next: Partial<AppearanceState>) {
  state = { ...state, ...next }
  for (const l of listeners) l()
}
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => void listeners.delete(l)
}

export function useAppearance() {
  return useSyncExternalStore(subscribe, () => state)
}

/** Re-renders with the resolved theme (Sistema follows the Mac). */
export function useResolvedDark() {
  return useAppearance().dark
}

// --- Token values read back from tokens.css. ---

/**
 * A token's value as tokens.css declares it for a theme, read from a hidden probe that carries its own
 * `data-theme`: the probe re-declares every token, so the values written on <html> don't leak into it.
 */
function tokenColor(name: string, dark: boolean): [number, number, number, number] {
  const raw = tokenValue(name, dark)
  const hex = /^#([0-9a-f]{6})$/i.exec(raw)
  if (hex) {
    const n = parseInt(hex[1]!, 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1]
  }
  const m = /rgba?\(([^)]+)\)/.exec(raw)
  const parts = m ? m[1]!.split(',').map((p) => Number(p.trim())) : [0, 0, 0, 1]
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0, parts[3] ?? 1]
}
const probes: { light?: HTMLElement; dark?: HTMLElement } = {}
/**
 * MX-93: tokens.css é estático, então cada tema é lido uma vez, num retrato de todas as variáveis da sonda. Ler a
 * sonda a cada token, entre as escritas que apply() faz no <html>, forçava recalcular o estilo do documento inteiro
 * (34 vezes no arranque; com a abertura na tela, isso atrasava o app). Nome fora do retrato: leitura direta.
 */
const tokenValues: { light?: Map<string, string>; dark?: Map<string, string> } = {}
function tokenValue(name: string, dark: boolean): string {
  const theme = dark ? 'dark' : 'light'
  const probe = probes[theme] ??= (() => {
    const el = document.createElement('div')
    el.dataset.theme = theme
    el.hidden = true
    el.setAttribute('aria-hidden', 'true')
    document.documentElement.append(el)
    return el
  })()
  const values = tokenValues[theme] ??= (() => {
    const style = getComputedStyle(probe)
    const all = new Map<string, string>()
    for (let i = 0; i < (style.length ?? 0); i++) {
      const property = style[i]
      if (property?.startsWith('--')) all.set(property.slice(2), style.getPropertyValue(property).trim())
    }
    return all
  })()
  let value = values.get(name)
  if (value === undefined) {
    value = getComputedStyle(probe).getPropertyValue(`--${name}`).trim()
    if (value) values.set(name, value)
  }
  return value
}
const rgb = (c: readonly number[]): RGB => [c[0]!, c[1]!, c[2]!]
const rgba = (c: readonly number[], a: number) => `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${Math.round(Math.min(1, Math.max(0, a)) * 1000) / 1000})`

// --- The wallpaper's picture. ---

export function wallpaperURL(id: WallpaperId, dark: boolean): string | null {
  if (id === 'none') return null
  if (id === 'custom') return state.customURL
  if (!offeredIn(id, dark)) return null
  const house = HOUSE_WALLPAPERS.find((w) => w.id === id)
  return house ? (dark ? house.dark : house.light) : null
}

/**
 * MX-79: a miniatura da grade de Aparência (64×42 px). Fundos da casa usam o WebP de 256 px em
 * `assets/wallpapers/thumbs/`; os de código (SVG) e a imagem própria sem miniatura guardada caem na imagem cheia.
 */
export function wallpaperThumbURL(id: WallpaperId, dark: boolean): string | null {
  if (id === 'none') return null
  if (id === 'custom') return state.customThumbURL ?? state.customURL
  if (!offeredIn(id, dark)) return null
  const house = HOUSE_WALLPAPERS.find((w) => w.id === id)
  if (!house) return null
  return (dark ? house.thumbDark : house.thumbLight) ?? (dark ? house.dark : house.light)
}

// The person's own picture lives in this app's IndexedDB (its own folder on the Mac), reduced to
// CUSTOM_MAX_SIDE: no file path from outside the app is kept.
const DB = 'donnabot-appearance'
function db() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const open = indexedDB.open(DB, 1)
    open.onupgradeneeded = () => open.result.createObjectStore('images')
    open.onsuccess = () => resolve(open.result)
    open.onerror = () => reject(open.error)
  })
}
// Queue before opening IDB: opening separate connections need not finish in request order. Explicit
// removals always run; a later valid import is queued after them, while a failed import cannot cancel
// them. Only imports are conditional on their intent still being current at transaction creation.
let imageWrites: Promise<void> = Promise.resolve()
/** A imagem cheia e a miniatura da grade viajam juntas: as duas chaves na mesma transação, ou nenhuma. */
function storeImages(full: Blob | null, thumb: Blob | null, current: () => boolean = () => true): Promise<boolean> {
  const write = imageWrites.then(async () => {
    const base = await db()
    try {
      if (!current()) return false
      await new Promise<void>((resolve, reject) => {
        const tx = base.transaction('images', 'readwrite')
        const store = tx.objectStore('images')
        if (full) {
          store.put(full, 'custom')
          if (thumb) store.put(thumb, 'custom-thumb')
        } else {
          store.delete('custom')
          store.delete('custom-thumb')
        }
        tx.oncomplete = () => resolve()
        tx.onabort = tx.onerror = () => reject(tx.error)
      })
      return true
    } finally {
      base.close()
    }
  })
  // A failed write is reported to its caller without blocking the next operation or a recovery read.
  imageWrites = write.then(() => {}, () => {})
  return write
}
interface StoredImages { full: Blob; thumb: Blob | null }
async function loadImage(): Promise<StoredImages | null> {
  // In particular, recovery after an invalid import must not read back a pending removal's old blob.
  await imageWrites
  const base = await db()
  const images = await new Promise<StoredImages | null>((resolve, reject) => {
    const store = base.transaction('images').objectStore('images')
    const get = (key: string) => new Promise<Blob | undefined>((res, rej) => {
      const req = store.get(key)
      req.onsuccess = () => res(req.result as Blob | undefined)
      req.onerror = () => rej(req.error)
    })
    void Promise.all([get('custom'), get('custom-thumb')]).then(([full, thumb]) => resolve(full ? { full, thumb: thumb ?? null } : null), reject)
  })
  base.close()
  return images
}

function setCustom(full: Blob | null, thumb: Blob | null = null) {
  if (state.customURL) URL.revokeObjectURL(state.customURL)
  if (state.customThumbURL) URL.revokeObjectURL(state.customThumbURL)
  setState({ customURL: full ? URL.createObjectURL(full) : null, customThumbURL: thumb ? URL.createObjectURL(thumb) : null })
}

/** Choosing and removing the picture: the latest intent wins, in the window and in storage (`intents.ts`). */
const customIntents = new Intents()
// pref notifies even for repeated Nenhum. Only wallpaper selections invalidate imports; accent,
// sliders, theme, and apply's asynchronous legibility measurements remain independent.
appearancePrefs.wallpaper.subscribe(() => { customIntents.begin() })

/**
 * Keeps a picture the person chose as the wallpaper: reduced, stored, and selected. Resolves `false`
 * when a later import, wallpaper selection or removal took over meanwhile.
 */
export async function chooseCustomWallpaper(file: File): Promise<boolean> {
  // Chromium can't decode HEIC: say so before anything changes (no intent, no decode).
  if (isHeic(file)) throw new Error(CUSTOM_ERRORS.heic)
  const intent = customIntents.begin()
  try {
    return await importCustom(file, intent)
  } catch {
    // A picture that can't be read leaves the stored one as it was: show it again if nothing newer came.
    if (customIntents.current(intent) && !state.customURL) void loadImage().then((stored) => { if (stored && customIntents.current(intent)) { setCustom(stored.full, stored.thumb); apply() } }).catch(() => {})
    throw new Error(CUSTOM_ERRORS.unreadable)
  }
}

async function importCustom(file: File, intent: number): Promise<boolean> {
  const bitmap = await createImageBitmap(file)
  if (!customIntents.current(intent)) {
    bitmap.close()
    return false
  }
  const scale = Math.min(1, CUSTOM_MAX_SIDE / Math.max(bitmap.width, bitmap.height))
  const canvas = document.createElement('canvas')
  canvas.width = Math.round(bitmap.width * scale)
  canvas.height = Math.round(bitmap.height * scale)
  canvas.getContext('2d')!.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
  // MX-79: a grade de Aparência não pode decodificar a imagem cheia para um cartão de 64×42 px.
  const thumbScale = Math.min(1, CUSTOM_THUMB_SIDE / Math.max(bitmap.width, bitmap.height))
  const thumbCanvas = document.createElement('canvas')
  thumbCanvas.width = Math.max(1, Math.round(bitmap.width * thumbScale))
  thumbCanvas.height = Math.max(1, Math.round(bitmap.height * thumbScale))
  thumbCanvas.getContext('2d')!.drawImage(bitmap, 0, 0, thumbCanvas.width, thumbCanvas.height)
  bitmap.close()
  const type = keepsAlpha(file) ? 'image/webp' : 'image/jpeg'
  const [blob, thumb] = await Promise.all([
    new Promise<Blob>((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error(CUSTOM_ERRORS.unreadable))), type, 0.88)),
    new Promise<Blob>((resolve, reject) => thumbCanvas.toBlob((b) => (b ? resolve(b) : reject(new Error(CUSTOM_ERRORS.unreadable))), 'image/webp', 0.8)),
  ])
  if (!(await storeImages(blob, thumb, () => customIntents.current(intent)))) return false
  if (!customIntents.current(intent)) return false
  setCustom(blob, thumb)
  appearancePrefs.wallpaper.set('custom')
  apply()
  return true
}

/** Forgets the person's own picture; the wallpaper goes back to Nenhum if it was in use. Wins over an import still in progress. */
export async function removeCustomWallpaper(): Promise<boolean> {
  customIntents.begin()
  // The window forgets it at once: a pending import can no longer select it.
  setCustom(null, null)
  if (appearancePrefs.wallpaper.get() === 'custom') appearancePrefs.wallpaper.set('none')
  apply()
  return storeImages(null, null)
}

// --- The floor: a bound that holds for any crop, scale or blur of the picture. ---

/** The glass's backdrop filter (theme.css `.glass-panel`): `saturate(1.4)`. */
const GLASS_SATURATE = 1.4
/** The strongest row tint over glass: `selected` (ink at 8% light, 10% dark); hovers and fields are lighter. */
const ROW_TINT = 0.1

/** The picture's per-channel range at full resolution, on the canvas color; null when it can't be read back. */
const ranges = new Map<string, Bounds | null>()
async function pictureBounds(url: string, ground: RGB): Promise<Bounds | null> {
  const key = `${url}|${ground.join(',')}`
  if (ranges.has(key)) return ranges.get(key)!
  let bounds: Bounds | null = null
  try {
    const img = new Image()
    img.src = url
    await img.decode()
    const canvas = document.createElement('canvas')
    canvas.width = img.naturalWidth
    canvas.height = img.naturalHeight
    const ctx = canvas.getContext('2d', { willReadFrequently: true })!
    ctx.drawImage(img, 0, 0)
    bounds = channelBounds(ctx.getImageData(0, 0, canvas.width, canvas.height).data, ground)
  } catch {
    // A picture the canvas can't read back keeps the conservative floor measured ahead of time.
  }
  ranges.set(key, bounds)
  return bounds
}

/**
 * The floor for a picture in a theme with a veil: every color the window can show under the glass lies in
 * the picture's channel box (cover, crop and blur only average pixels), the veil moves the whole box, and
 * the glass's saturation is bounded on the box's corners. The glass must keep primary text (`ink`) at 4.5:1
 * on every layer that carries text (sidebar, conversation plate, bubbles), even under a row tint; secondary and
 * tertiary text adapt their own color over it (`writeGlass`). D169 (28/10): it used to hold `ink-2`, which
 * kept every house wallpaper at 0.88–0.96 and the light surfaces opaque whatever Transparência said.
 */
export function floorFor(bounds: Bounds, dark: boolean, veil: number, tokens: (name: string) => RGB) {
  return safeFloor(shownWorst(bounds, dark, veil, tokens), sharedLayers(tokens, dark), tokens('ink'))
}

/** The color under the glass that is worst for text, after the veil and the glass's saturation. */
function shownWorst(bounds: Bounds, dark: boolean, veil: number, tokens: (name: string) => RGB) {
  return worstOf(saturatedBounds(veiledBounds(bounds, tokens('wallpaper-veil'), veil), GLASS_SATURATE), dark)
}

/** Donna's card, selected with its inner button hovered: `selected` + `field` stacked (12.6% light, 16.3% dark). */
const DONNA_TINT = { light: 0.126, dark: 0.163 }

function sharedLayers(tokens: (name: string) => RGB, dark: boolean) {
  const layers = [
    // The sidebar's rows: hover, fields, the selected row and Donna's card (the strongest) tint the glass under their text.
    { color: tokens('glass-sidebar'), offset: 0, tint: { color: tokens('ink'), alpha: Math.max(ROW_TINT, DONNA_TINT[dark ? 'dark' : 'light']) } },
    { color: tokens('canvas'), offset: 0 },
    { color: tokens('glass-raised'), offset: 0.02 },
    { color: tokens('glass-strong'), offset: 0.1 },
  ]
  return layers
}

/**
 * D169: the floors in use: the shared one (every glass that carries text) and the details panel's, which
 * never goes below the shared one. Until the picture is read, the details floor is taken on the extreme
 * (black in the light theme, white in the dark), so it can only be higher than the measured one.
 */
interface Floors {
  shared: number
  details: number
}
function detailsFloor(shared: number, worst: RGB, dark: boolean) {
  const tokens = (name: string) => rgb(tokenColor(name, dark))
  try {
    return Math.max(shared, safeFloor(worst, [detailsLayer(tokens)], tokens('ink')))
  } catch {
    // A theme where even the opaque layer can't carry the text keeps the panel opaque, never a throw.
    return 1
  }
}
const extreme = (dark: boolean): RGB => (dark ? [255, 255, 255] : [0, 0, 0])

/** `legibleOn`, never a throw: a ground so extreme that no shade passes gets the theme's end color (black in light). */
function textOn(color: RGB, grounds: readonly RGB[], dark: boolean, ratio: number): RGB {
  try {
    return legibleOn(color, grounds, dark, ratio)
  } catch {
    return extreme(dark)
  }
}

const floors = new Map<string, Floors>()
async function measureFloor(url: string, dark: boolean, veil: number, fallback: Floors): Promise<Floors> {
  const key = floorKey(url, dark, veil)
  const known = floors.get(key)
  if (known !== undefined) return known
  const bounds = await pictureBounds(url, rgb(tokenColor('canvas', dark)))
  const tokens = (name: string) => rgb(tokenColor(name, dark))
  const shared = bounds ? floorFor(bounds, dark, veil, tokens) : fallback.shared
  const measured = bounds ? { shared, details: detailsFloor(shared, shownWorst(bounds, dark, veil, tokens), dark) } : fallback
  floors.set(key, measured)
  return measured
}
/** Blur doesn't move the bound (it only averages), so the floor depends on the picture, theme and veil. */
const floorKey = (url: string, dark: boolean, veil: number) => `${url}|${dark}|${veil}`

// --- Writing it all on <html>. ---

/** Every apply is an intent: a measurement started by an older one never writes (F1, D165 revisão A). */
const applies = new Intents()

/** Writes the theme, accent, wallpaper and glass on <html>. Safe to call often; the latest call wins. */
let spaceAppearance: SpaceAppearance | null = null
/** E2: só apresentação da janela; nunca grava a escolha do Space nas preferências do Pessoal. */
export function setSpaceAppearance(value: SpaceAppearance | null) { spaceAppearance = value; apply() }
export function apply() {
  const mine = applies.begin()
  const root = document.documentElement
  const dark = resolvedDark()
  root.dataset.theme = dark ? 'dark' : 'light'
  root.dataset.toquesMexerica = appearancePrefs.mexericaTouches.get() ? 'on' : 'off'
  root.dataset.accent = appearancePrefs.accent.get()
  const font = fontStack(appearancePrefs.font.get())
  if (font) root.style.setProperty('--font-sans', font)
  else root.style.removeProperty('--font-sans')
  for (const [name, value] of tagColorVars(readTagColors(appearancePrefs.sidebarTagColors.get()))) root.style.setProperty(name, value)

  const id = spaceAppearance?.wallpaper ?? appearancePrefs.wallpaper.get()
  const url = wallpaperURL(id, dark)
  const blur = Math.min(Math.max(spaceAppearance?.blur ?? appearancePrefs.blur.get(), 0), BLUR_MAX)
  const veil = Math.min(Math.max((spaceAppearance ? dark ? spaceAppearance.veilDark : spaceAppearance.veilLight : appearancePrefs.veil.get()) ?? VEIL_DEFAULT[dark ? 'dark' : 'light'], 0), VEIL_MAX)
  const general = spaceAppearance?.transparency ?? appearancePrefs.transparency.get()
  const mode = appearancePrefs.transparencyMode.get()
  const own = appearancePrefs.transparencyAreas.get()
  const chosen = glassAlpha(general)
  // D169: each glass area's picked opacity (the general one in Geral, or an area without its own) and
  // the conversation's plate, which needs no floor because no text lies on it.
  const pick = (area: TransparencyArea) => glassAlpha(spaceAppearance ? general : areaChoice(area, mode, general, own))
  const plate = conversationPlate(areaChoice('conversation', mode, general, own))
  const opts = { moreContrast: moreContrast.get(), reduceTransparency: reduceTransparency.get() }
  const style = root.style
  // Papéis exclusivos do hover D530. Não sobrescrevem --hover, --accent nem as guardas existentes.
  const hoverInk = toquesHoverInk(rgb(tokenColor(`accent-${appearancePrefs.accent.get()}`, dark)), rgb(tokenColor('surface', dark)), dark)
  style.setProperty('--mx-hover-fill', TOQUES_HOVER_FILL)
  style.setProperty('--mx-hover-ink', `rgb(${hoverInk.join(', ')})`)
  // Text on the covered layers must be at least as legible as the secondary text in use (ink-2, or its darker/
  // lighter version over clear glass), even on opaque selected rows. Original accents/status tokens remain the
  // fills and focus colors. Written on every path (no wallpaper, reduced transparency, a cached floor); an
  // accent change takes effect now.
  const writeText = (reference: RGB) => {
    for (const name of ['accent', 'attention', 'danger', 'success'] as const) {
      const source = name === 'accent' ? `accent-${appearancePrefs.accent.get()}` : name
      const color = readableForeground(rgb(tokenColor(source, dark)), reference, dark)
      style.setProperty(`--legible-${name}`, `rgb(${color.join(', ')})`)
    }
  }
  if (state.reducedTransparency !== opts.reduceTransparency || state.moreContrast !== opts.moreContrast) {
    setState({ reducedTransparency: opts.reduceTransparency, moreContrast: opts.moreContrast })
  }
  const glassNames = ['glass-sidebar', 'glass-rail', 'glass-raised', 'glass-strong'] as const
  const areaNames = ['--glass-composer', '--glass-details', '--glass-conversation'] as const

  // Donna's two nested backgrounds stack: selected (8%/10%) + hovered field (5%/7%) = 12.6%/16.3%.
  // Its secondary and semantic text get their own bound, without changing those fills or the shared
  // glass floor. Include the full channel box until the picture has been read, just as the fallback does.
  const writeDonna = (sidebar: RGB) => {
    const ink = rgb(tokenColor('ink', dark))
    const selected = tokenColor('selected', dark)[3]
    const field = tokenColor('field', dark)[3]
    const ground = over(ink, 1 - (1 - selected) * (1 - field), sidebar)
    for (const name of ['secondary', 'accent', 'attention', 'danger', 'success'] as const) {
      const source = name === 'secondary' ? 'ink-2' : name === 'accent' ? `accent-${appearancePrefs.accent.get()}` : name
      // The floor holds primary text on this very ground (sharedLayers), so the end color always passes; never a throw.
      const color = textOn(rgb(tokenColor(source, dark)), [ground], dark, 4.5)
      style.setProperty(`--donna-text-${name}`, `rgb(${color.join(', ')})`)
    }
  }

  // The effective opacity of the general glass (bubbles, pills, cards, other screens' plate) and of the
  // three glass areas. In Geral every area equals the general one, so these write what they did before.
  type Glasses = { general: number } & Record<'sidebar' | 'details' | 'composer', number>
  const all = (glass: number): Glasses => ({ general: glass, sidebar: glass, details: glass, composer: glass })
  const areasOf = (g: Glasses, measured: boolean): AppearanceState['areas'] => {
    const area = (name: 'sidebar' | 'details' | 'composer') => ({ glass: g[name], chosen: pick(name), held: measured && g[name] > pick(name) + 0.001 })
    return { sidebar: area('sidebar'), details: area('details'), composer: area('composer'), conversation: { glass: plate, chosen: plate, held: false } }
  }

  const writeGlass = (g: Glasses) => {
    const sidebar = tokenColor('glass-sidebar', dark)
    const raised = tokenColor('glass-raised', dark)
    const canvas = tokenColor('canvas', dark)
    style.setProperty('--glass-sidebar', rgba(sidebar, g.sidebar))
    style.setProperty('--glass-raised', rgba(raised, g.general + 0.02))
    style.setProperty('--glass-strong', rgba(tokenColor('glass-strong', dark), g.general + 0.1))
    style.setProperty('--glass-plate', rgba(canvas, g.general))
    style.setProperty('--glass-composer', rgba(raised, g.composer + 0.02))
    style.setProperty('--glass-details', rgba(raised, g.details + 0.02))
    style.setProperty('--glass-conversation', rgba(canvas, plate))
    const bounds = url ? ranges.get(`${url}|${rgb(tokenColor('canvas', dark)).join(',')}`) : null
    const worst = bounds
      ? worstOf(saturatedBounds(veiledBounds(bounds, rgb(tokenColor('wallpaper-veil', dark)), veil), GLASS_SATURATE), dark)
      : dark ? [255, 255, 255] as const : [0, 0, 0] as const
    const rail = tokenColor('glass-rail', dark)
    const railTint = Math.max(...['hover', 'field', 'selected'].map((name) => tokenColor(name, dark)[3]))
    const railAlpha = opts.reduceTransparency ? 1 : controlGlassAlpha(g.sidebar * 0.7, worst,
      { color: rgb(rail), offset: 0, tint: { color: rgb(tokenColor('ink', dark)), alpha: railTint } }, rgb(tokenColor('ink', dark)))
    style.setProperty('--glass-rail', rgba(rail, railAlpha))
    writeDonna(over(rgb(sidebar), g.sidebar, worst))
    // D169 (pedido do dono 28/10): the glass is held only for primary text; secondary (4.5:1) and tertiary (3:1)
    // text keep their contrast on every text-carrying layer at the opacity in use by their own color.
    const tokens = (name: string) => rgb(tokenColor(name, dark))
    const layers: [GlassLayer, number][] = [
      [{ color: rgb(sidebar), offset: 0, tint: { color: tokens('ink'), alpha: Math.max(ROW_TINT, DONNA_TINT[dark ? 'dark' : 'light']) } }, g.sidebar],
      [{ color: rgb(canvas), offset: 0 }, g.general],
      [{ color: rgb(raised), offset: 0.02 }, g.general],
      [{ color: tokens('glass-strong'), offset: 0.1 }, g.general],
      [detailsLayer(tokens), g.details],
      [{ color: rgb(raised), offset: 0.02 }, g.composer],
    ]
    const grounds = layers.map(([layer, alpha]) => layerGround(worst, layer, alpha))
    const secondary = textOn(tokens('ink-2'), grounds, dark, 4.5)
    style.setProperty('--ink-2', `rgb(${secondary.join(', ')})`)
    style.setProperty('--ink-3', `rgb(${textOn(tokens('ink-3'), grounds, dark, 3).join(', ')})`)
    // R1-2 (#524): marcas não lidas da barrinha, sobre o véu `glass-raised`: o accent escolhido, a 3:1 como o ink-3.
    style.setProperty('--accent-marca', `rgb(${textOn(tokens(`accent-${appearancePrefs.accent.get()}`), grounds, dark, 3).join(', ')})`)
    writeText(secondary)
  }

  if (!url) {
    delete root.dataset.wallpaper
    for (const p of ['--wp-image', '--wp-blur', '--wallpaper-veil', '--glass-plate', '--ink-2', '--ink-3', '--accent-marca', ...areaNames]) style.removeProperty(p)
    writeText(rgb(tokenColor('ink-2', dark)))
    // No wallpaper: the tokens' own glass, made opaque when the system asks for less transparency.
    if (opts.reduceTransparency) writeGlass(all(1))
    else {
      for (const name of glassNames) style.removeProperty(`--${name}`)
      writeDonna(rgb(tokenColor('surface', dark)))
    }
    const shown = opts.reduceTransparency ? all(1) : { general: chosen, sidebar: pick('sidebar'), details: pick('details'), composer: pick('composer') }
    setState({ dark, floor: 0, glass: shown.general, chosen, held: false, areas: areasOf(shown, false) })
    return
  }

  root.dataset.wallpaper = id
  style.setProperty('--wp-image', `url("${url}")`)
  style.setProperty('--wp-blur', `${blur}px`)
  style.setProperty('--wallpaper-veil', rgba(tokenColor('wallpaper-veil', dark), veil))
  const house = HOUSE_WALLPAPERS.find((w) => w.id === id)
  // A picture of their own, until read back: opaque glass (the details panel too).
  const shared = house ? house.floor[dark ? 'dark' : 'light'] : 1
  const fallback: Floors = { shared, details: shared >= 1 ? 1 : detailsFloor(shared, extreme(dark), dark) }
  // Until the floor is measured, the conservative one holds: never a frame of illegible glass.
  const known = floors.get(floorKey(url, dark, veil))
  const glasses = (f: Floors): Glasses => ({
    general: effectiveGlass(chosen, f.shared, opts),
    sidebar: effectiveGlass(pick('sidebar'), f.shared, opts),
    details: effectiveGlass(pick('details'), f.details, opts),
    composer: effectiveGlass(pick('composer'), f.shared, opts),
  })
  const first = glasses(known ?? fallback)
  writeGlass(first)
  setState({ dark, floor: (known ?? fallback).shared, glass: first.general, chosen, held: first.general > chosen + 0.001, areas: areasOf(first, true) })
  if (known !== undefined) return
  void measureFloor(url, dark, veil, fallback).then((floor) => {
    if (!applies.current(mine)) return
    const g = glasses(floor)
    writeGlass(g)
    setState({ floor: floor.shared, glass: g.general, held: g.general > chosen + 0.001, areas: areasOf(g, true) })
  })
}

let started = false

/** Applies the appearance now and keeps it current: preferences (this window or another), the system's theme, contrast and transparency. */
export function startAppearance() {
  apply()
  if (started) return
  started = true
  for (const p of Object.values(appearancePrefs)) p.subscribe(apply)
  for (const q of [systemDark, reduceTransparency, moreContrast]) q.list.addEventListener('change', apply)
  // A missed change while the window was hidden never outlives the next look at it (lib/theme.ts).
  document.addEventListener('visibilitychange', apply)
  window.addEventListener('focus', apply)
  if (typeof indexedDB !== 'undefined') {
    // The stored picture comes back unless the person already chose or removed one meanwhile.
    const intent = customIntents.begin()
    customLoaded = loadImage().then((stored) => {
      if (!stored || !customIntents.current(intent)) return
      setCustom(stored.full, stored.thumb)
      apply()
    }).catch(() => {})
  }
}

let customLoaded: Promise<void> = Promise.resolve()

/**
 * aparencia-01: resolves once the person's own picture is back from storage (or after `timeout` ms), so the
 * window's first frame already has it instead of the opaque app turning into glass a moment later. Immediate
 * with any other wallpaper.
 */
export function appearanceReady(timeout = 300): Promise<void> {
  if (appearancePrefs.wallpaper.get() !== 'custom' || state.customURL) return Promise.resolve()
  return Promise.race([customLoaded, new Promise<void>((resolve) => setTimeout(resolve, timeout))])
}
