// DON-903 (docs/guides/idiomas.md): os idiomas do app, a direção e a sugestão pelo idioma do sistema.
// Sem DOM nem Electron: o processo principal usa `sugerirIdioma` com app.getPreferredSystemLanguages()
// e o renderer usa a mesma regra com navigator.languages fora do Electron (demo). Nada sai do computador.

/** Idiomas do app (D363). pt-BR é a fonte dos catálogos (toda chave nasce nele). */
export const APP_LANGUAGES = ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const
export type AppLanguage = (typeof APP_LANGUAGES)[number]
/**
 * D454/D495: o idioma padrão do app é o inglês. Sem escolha salva, o app fala inglês; quem escolheu um idioma
 * continua nele. Também é o primeiro fallback de um texto sem tradução, antes da fonte (`SOURCE_LANGUAGE`).
 */
export const FALLBACK_LANGUAGE: AppLanguage = 'en'
/** A fonte dos catálogos: o último fallback de um texto, quando nem o idioma nem o inglês o têm. */
export const SOURCE_LANGUAGE: AppLanguage = 'pt-BR'

export function isAppLanguage(value: unknown): value is AppLanguage {
  return typeof value === 'string' && (APP_LANGUAGES as readonly string[]).includes(value)
}

/** Escrita da direita para a esquerda só no árabe. */
export function direcaoDe(idioma: AppLanguage): 'ltr' | 'rtl' {
  return idioma === 'ar-AE' ? 'rtl' : 'ltr'
}

/** Etiquetas BCP 47 simples (letras, dígitos e hífen): o resto da lista do sistema é ignorado. */
const TAG = /^[A-Za-z]{2,3}(?:[-_][A-Za-z0-9]{1,8})*$/

/** Chinês tradicional (escrita Hant ou regiões que a usam) não recebe o simplificado como sugestão. */
function chinesSimplificado(partes: string[]): boolean {
  const resto = partes.slice(1).map((p) => p.toLowerCase())
  if (resto.includes('hans')) return true
  if (resto.includes('hant') || resto.some((p) => p === 'tw' || p === 'hk' || p === 'mo')) return false
  return true
}

/**
 * O primeiro idioma do sistema que o app tem: `pt-*` → pt-BR, `en-*` → en, `es-*` → es, `fr-*` → fr,
 * `zh` simplificado → zh-CN, `ar-*` → ar-AE. `null` quando nenhum serve (a tela começa em inglês, o padrão da
 * D454/D495, sem dizer que foi sugerido). A sugestão só pré-seleciona; a pessoa confirma.
 */
export function sugerirIdioma(preferidos: readonly unknown[]): AppLanguage | null {
  for (const raw of preferidos.slice(0, 16)) {
    if (typeof raw !== 'string' || raw.length > 35 || !TAG.test(raw)) continue
    const partes = raw.split(/[-_]/)
    const base = partes[0]!.toLowerCase()
    if (base === 'pt') return 'pt-BR'
    if (base === 'en' || base === 'es' || base === 'fr') return base
    if (base === 'ar') return 'ar-AE'
    if (base === 'zh' && chinesSimplificado(partes)) return 'zh-CN'
  }
  return null
}
