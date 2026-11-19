// D454 (fatias 8b em diante) e D507 (padrão único): uma `Frase` é dado, a chave tipada do catálogo com os parâmetros,
// ainda sem idioma. Lojas, regras e estados guardam a frase; a tela traduz ao desenhar com `textoDe(frase, idioma)`,
// função pura (lição da R1 do #572: trocar o idioma muda também avisos e erros). Sem closure: a frase pode ser clonada,
// serializada e comparada como qualquer dado.
// A forma é a mesma da `Mensagem` do #572 (`views/fork/bot-settings/mensagem.ts`: `{ chave, params } | { texto }`):
// toda `Mensagem` já é uma `Frase`, uma interface só.
import { traduzir, traduzirPlural, type ChavePlural, type ChaveTexto, type NomesDe, type NomesPluralDe } from './index.ts'
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { isolado } from '../../../shared/bidi.ts'
import { registrarTextoCru } from './erro-frase.ts'

/** Valor de parâmetro: texto ou número são dados (nome, Hub, resposta do Hub); `Frase` é uma frase nossa dentro de outra. */
export type Valor = string | number | Frase
type Parametros = Readonly<Record<string, Valor>>

export type Frase =
  | { readonly chave: ChaveTexto; readonly params?: Parametros }
  | { readonly plural: ChavePlural; readonly n: number; readonly params?: Parametros }
  | { readonly juntas: readonly Frase[] }
  | { readonly texto: string }

/** O resto das chaves `P…` que não pedem parâmetro (atalho de tela: `L('fechar')` para `skills.detalhes.fechar`). */
export type SufixoSemParams<P extends string> = { [K in ChaveTexto]: K extends `${P}${infer S}` ? ([NomesDe<K>] extends [never] ? S : never) : never }[ChaveTexto]

/** Os parâmetros que a chave pede, conferidos pelo tipo (os mesmos nomes do texto pt-BR). */
type ParamsDe<N extends string> = [N] extends [never] ? [] : [params: Readonly<Record<N, Valor>>]

/** A frase da `chave`. */
export function frase<K extends ChaveTexto>(chave: K, ...[params]: ParamsDe<NomesDe<K>>): Frase {
  return params ? { chave, params } : { chave }
}

/** O plural do catálogo (`base.one`, `base.other`…) para `n`; o `{n}` sai no formato do idioma. */
export function plural<B extends ChavePlural>(base: B, n: number, ...[params]: ParamsDe<NomesPluralDe<B>>): Frase {
  return params ? { plural: base, n, params } : { plural: base, n }
}

/** Texto que veio pronto do Hub (dado): igual em todo idioma. */
export const cru = (texto: string): Frase => ({ texto })

/** Frases inteiras, cada uma com a pontuação dela, juntas por espaço (nunca pedaços de uma frase). */
export const juntas = (...partes: (Frase | null | undefined)[]): Frase => ({ juntas: partes.filter((p): p is Frase => !!p) })

const so = (v: object, chaves: string[]) => Object.keys(v).every((k) => chaves.includes(k))
export const ehFrase = (v: unknown): v is Frase => {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false
  const o = v as Record<string, unknown>
  if (typeof o.chave === 'string') return so(o, ['chave', 'params'])
  if (typeof o.plural === 'string') return typeof o.n === 'number' && so(o, ['plural', 'n', 'params'])
  if (Array.isArray(o.juntas)) return so(o, ['juntas'])
  return typeof o.texto === 'string' && so(o, ['texto'])
}

const traduzirLivre = traduzir as unknown as (idioma: AppLanguage, chave: ChaveTexto, params?: Record<string, string | number>) => string
const pluralLivre = traduzirPlural as unknown as (idioma: AppLanguage, base: ChavePlural, n: number, params?: Record<string, string | number>) => string

/** Parâmetro texto é dado e entra isolado da frase, também em idioma LTR; parâmetro `Frase` sai no mesmo idioma. */
function prontos(params: Parametros | undefined, idioma: AppLanguage) {
  if (!params) return undefined
  const r: Record<string, string | number> = {}
  // Texto pronto (`cru`, a `{ texto }` do #572) também é dado: entra isolado como o texto puro (D454, fatia 9c).
  for (const [nome, v] of Object.entries(params)) r[nome] = typeof v === 'string' ? isolado(v) : typeof v === 'number' ? v : 'texto' in v ? isolado(v.texto) : textoDe(v, idioma)
  return r
}

/** O texto da frase no idioma da tela (sem frase, nada). */
export function textoDe(f: Frase, idioma: AppLanguage): string
export function textoDe(f: Frase | null | undefined, idioma: AppLanguage): string | undefined
export function textoDe(f: Frase | null | undefined, idioma: AppLanguage): string | undefined {
  if (!f) return undefined
  if ('texto' in f) return f.texto
  if ('juntas' in f) return f.juntas.map((p) => textoDe(p, idioma)).join(' ')
  if ('plural' in f) return pluralLivre(idioma, f.plural, f.n, prontos(f.params, idioma))
  return traduzirLivre(idioma, f.chave, prontos(f.params, idioma))
}

// A classe vive em módulo próprio, sem catálogos (ver `erro-frase.ts`); aqui só se liga a mensagem crua em inglês.
export { ErroFrase } from './erro-frase.ts'
registrarTextoCru((f) => textoDe(f, 'en'))
