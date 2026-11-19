// DON-903 (docs/guides/idiomas.md): tradução tipada, plural e formatação, sem DOM nem dependência.
// A ligação com a preferência e o React fica em estado.ts; aqui só regras puras, testáveis no Node.
import { direcaoDe, FALLBACK_LANGUAGE, isAppLanguage, SOURCE_LANGUAGE, type AppLanguage } from '../../../shared/fork-idioma.ts'
import { arAE } from './ar-AE/index.ts'
import { en } from './en/index.ts'
import { es } from './es/index.ts'
import { fr } from './fr/index.ts'
import { ptBR } from './pt-BR/index.ts'
import { zhCN } from './zh-CN/index.ts'

type Fonte = typeof ptBR
type ChaveFonte = keyof Fonte

/** Categorias CLDR (Intl.PluralRules). Cada idioma usa as suas: ar todas, zh só `other`, pt/en/es/fr `one` e `other`. */
export const CATEGORIAS = ['zero', 'one', 'two', 'few', 'many', 'other'] as const
export type CategoriaPlural = (typeof CATEGORIAS)[number]
/** A base de um plural: a chave pt-BR `x.other` (com `x.one` ao lado) dá a base `x`. */
export type ChavePlural = { [K in ChaveFonte]: K extends `${infer B}.other` ? B : never }[ChaveFonte]
type VariantePlural = `${ChavePlural}.${CategoriaPlural}`
/** Chave comum: existe igual nos seis idiomas. As variantes de plural ficam com `t.plural`. */
export type ChaveTexto = Exclude<ChaveFonte, VariantePlural>
/**
 * Todo idioma tem todas as chaves comuns e o `other` de cada plural; as demais categorias são as do idioma
 * (tools/textos-gate.ts confere categorias e parâmetros).
 */
export type Catalogo = { readonly [K in ChaveTexto]: string } & { readonly [K in `${ChavePlural}.other`]: string } & { readonly [K in VariantePlural]?: string }
/** Um módulo de tradução (`en/whatsapp/prazo.ts`…): as chaves do mesmo módulo pt-BR `F`; das variantes de plural, só `other` é obrigatória. */
type BasesDe<F> = { [K in keyof F]: K extends `${infer B}.other` ? B : never }[keyof F]
export type Traducao<F> = { readonly [K in keyof F as K extends `${BasesDe<F> & string}.${Exclude<CategoriaPlural, 'other'>}` ? never : K]: string } & { readonly [K in `${BasesDe<F> & string}.${CategoriaPlural}`]?: string }

/** Os nomes entre chaves de um texto pt-BR: `'Passo {atual} de {total}'` → `'atual' | 'total'`. */
type NomesParametros<S extends string> = S extends `${string}{${infer P}}${infer R}` ? P | NomesParametros<R> : never
type Params<S extends string> = [NomesParametros<S>] extends [never] ? [] : [params: Record<NomesParametros<S>, string | number>]
export type ParametrosDe<K extends ChaveTexto> = Params<Fonte[K]>
/** Os nomes dos parâmetros de uma chave (para quem guarda a chave como dado, D507). */
export type NomesDe<K extends ChaveTexto> = NomesParametros<Fonte[K]>
/** Num plural, `{n}` vem da contagem; os outros parâmetros do texto `other` pt-BR são exigidos. */
type TextoOther<B extends ChavePlural> = Fonte[`${B}.other` & ChaveFonte] & string
export type ParametrosPlural<B extends ChavePlural> = [Exclude<NomesParametros<TextoOther<B>>, 'n'>] extends [never]
  ? []
  : [params: Record<Exclude<NomesParametros<TextoOther<B>>, 'n'>, string | number>]
/** Os nomes dos parâmetros de um plural, fora o `{n}` (D507). */
export type NomesPluralDe<B extends ChavePlural> = Exclude<NomesParametros<TextoOther<B>>, 'n'>

export const CATALOGOS: Readonly<Record<AppLanguage, Catalogo>> = { 'pt-BR': ptBR, en, es, fr, 'zh-CN': zhCN, 'ar-AE': arAE }

// Isolamento bidi (FSI … PDI): num texto da direita para a esquerda, nome, código ou número inserido não
// reordena a frase em volta. Em idiomas da esquerda para a direita, nada é acrescentado.
const FSI = '⁨'
const PDI = '⁩'

function interpolar(idioma: AppLanguage, texto: string, valores: Record<string, string | number> | undefined) {
  if (!valores) return texto
  const isolar = direcaoDe(idioma) === 'rtl'
  return texto.replace(/\{(\w+)\}/g, (inteiro, nome: string) => {
    if (!(nome in valores)) return inteiro
    const v = String(valores[nome])
    return isolar ? FSI + v + PDI : v
  })
}

/** D454/D495: a cadeia de um texto sem tradução: o idioma, depois o inglês (o padrão), depois a fonte pt-BR. */
export const CADEIA_FALLBACK = (idioma: AppLanguage): readonly AppLanguage[] => [...new Set([idioma, FALLBACK_LANGUAGE, SOURCE_LANGUAGE])]

/** O primeiro texto não vazio de `chave` pela cadeia, em catálogos dados (puro; os testes usam catálogos próprios). */
export function textoEm(catalogos: Readonly<Record<AppLanguage, Readonly<Record<string, string | undefined>>>>, idioma: AppLanguage, chave: string): string | undefined {
  for (const i of CADEIA_FALLBACK(idioma)) {
    const texto = catalogos[i]?.[chave]
    if (texto) return texto
  }
  return undefined
}

function bruto(idioma: AppLanguage, chave: string): string {
  return textoEm(CATALOGOS as Readonly<Record<AppLanguage, Readonly<Record<string, string | undefined>>>>, idioma, chave)!
}

/** O texto de `chave` em `idioma`, com os parâmetros; sem tradução (texto vazio), o inglês e depois o pt-BR. */
export function traduzir<K extends ChaveTexto>(idioma: AppLanguage, chave: K, ...[params]: ParametrosDe<K>): string {
  return interpolar(idioma, bruto(idioma, chave), params as Record<string, string | number> | undefined)
}

/** O locale de formatação e de plural do idioma (`en` formata como en-US). */
export function localeDe(idioma: AppLanguage): string {
  return idioma === 'en' ? 'en-US' : idioma
}

/**
 * Plural de uma contagem: com n = 0 e `base.zero` no catálogo do idioma, essa forma (D507); senão a categoria vem
 * de Intl.PluralRules do idioma; sem essa categoria no catálogo, `base.other` do idioma; sem tradução, o inglês e
 * depois o pt-BR. `{n}` é a contagem formatada no idioma.
 */
export function traduzirPlural<B extends ChavePlural>(idioma: AppLanguage, base: B, n: number, ...[params]: ParametrosPlural<B>): string {
  return pluralEm(CATALOGOS as Readonly<Record<AppLanguage, Readonly<Record<string, string | undefined>>>>, idioma, base, n, params as Record<string, string | number> | undefined)
}

/** A regra de `traduzirPlural` sobre catálogos dados (pura; os testes usam catálogos próprios). */
export function pluralEm(catalogos: Readonly<Record<AppLanguage, Readonly<Record<string, string | undefined>>>>, idioma: AppLanguage, base: string, n: number, params?: Record<string, string | number>): string {
  const forma = (i: AppLanguage) => {
    const cat = catalogos[i]
    // D507: o zero explícito vale em qualquer idioma (ar já o seleciona pelo CLDR); sem `.zero`, segue o CLDR.
    return (n === 0 ? cat?.[`${base}.zero`] : undefined) || cat?.[`${base}.${new Intl.PluralRules(localeDe(i)).select(n)}`] || cat?.[`${base}.other`]
  }
  const texto = CADEIA_FALLBACK(idioma).map(forma).find(Boolean) || base
  return interpolar(idioma, texto, { ...params, n: formatarNumero(idioma, n) })
}

/**
 * Categorias de plural do idioma para o gate. `obrigatorias`: todas as que alguma contagem inteira não negativa
 * alcança (amostra 0–1000 e potências de 10 até 10¹²): ar-AE as seis; zh-CN só `other`; en `one`/`other`;
 * pt-BR, es e fr também `many`, a forma real dos milhões ("1 milhão de bots"). Nada que o idioma não tenha é
 * exigido, e nada que ele tenha fica escondido. `permitidas`: as que Intl lista.
 */
export function categoriasPlural(idioma: AppLanguage): { obrigatorias: CategoriaPlural[]; permitidas: CategoriaPlural[] } {
  const regras = new Intl.PluralRules(localeDe(idioma))
  const vistas = new Set<string>()
  for (let n = 0; n <= 1000; n++) vistas.add(regras.select(n))
  for (let e = 4; e <= 12; e++) for (const m of [1, 2, 5]) vistas.add(regras.select(m * 10 ** e))
  // Na ordem canônica do CLDR: a ordem de `pluralCategories` muda entre versões do Node.
  const listadas = new Set<string>(regras.resolvedOptions().pluralCategories)
  // D507: `zero` é sempre permitida (forma natural do zero, "Nenhum bot"), mas só ar-AE a exige.
  const permitidas = CATEGORIAS.filter((c) => c === 'zero' || listadas.has(c))
  return { obrigatorias: permitidas.filter((c) => vistas.has(c)), permitidas }
}

export function formatarData(idioma: AppLanguage, data: Date | number, opcoes?: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(localeDe(idioma), opcoes).format(data)
}

export function formatarNumero(idioma: AppLanguage, valor: number, opcoes?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(localeDe(idioma), opcoes).format(valor)
}

/** A moeda é a do dado (ISO 4217, ex. `BRL`, `USD`), nunca deduzida do idioma; só o formato segue o idioma. */
export function formatarMoeda(idioma: AppLanguage, valor: number, moeda: string, opcoes?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(localeDe(idioma), { ...opcoes, style: 'currency', currency: moeda }).format(valor)
}

export interface Tradutor {
  <K extends ChaveTexto>(chave: K, ...params: ParametrosDe<K>): string
  /** Contagem inteira não negativa: `t.plural('base', n)`; o texto usa `{n}`. */
  plural<B extends ChavePlural>(base: B, n: number, ...params: ParametrosPlural<B>): string
}

/** `t` de um idioma fixo (puro; o gancho `useT` de estado.ts usa este com o idioma atual). */
export function tradutor(idioma: AppLanguage): Tradutor {
  const t = ((chave, ...params) => traduzir(idioma, chave, ...params)) as Tradutor
  t.plural = (base, n, ...params) => traduzirPlural(idioma, base, n, ...params)
  return t
}

/** Os atributos de uma superfície já traduzida: o idioma escolhido e a direção dele (puro; reexportado em estado.ts). */
export function superficieIdioma(idioma: AppLanguage): { lang: AppLanguage; dir: 'ltr' | 'rtl' } {
  return { lang: idioma, dir: direcaoDe(idioma) }
}

/**
 * O idioma que estado.ts aplica ao `<html>`, lido sem tocar em preferências (módulos que não podem ler preferência,
 * como a porta da conta, e textos montados fora do React). Sem DOM (testes), o padrão.
 */
export function idiomaDoDocumento(): AppLanguage {
  const lang = globalThis.document?.documentElement?.lang
  return lang && isAppLanguage(lang) ? lang : FALLBACK_LANGUAGE
}
