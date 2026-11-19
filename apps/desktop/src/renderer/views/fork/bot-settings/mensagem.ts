// Fatia 3, R1 do #572: uma mensagem de erro guardada como CHAVE do catálogo (com parâmetros), traduzida só na
// renderização; trocar o idioma com o erro à vista troca o texto. `texto` fica para o que já chega pronto (Hub, rede).
import { FALLBACK_LANGUAGE, isAppLanguage } from '../../../../shared/fork-idioma.ts'
import { tradutor, type ChaveTexto, type Tradutor } from '../../../lib/fork-i18n/index.ts'

export type Mensagem = { chave: ChaveTexto; params?: Record<string, string | number> } | { texto: string }

/** O texto da mensagem no idioma de `t`. */
export function textoDaMensagem(t: Tradutor, m: Mensagem): string {
  return 'chave' in m ? (t as unknown as (c: ChaveTexto, p?: Record<string, string | number>) => string)(m.chave, m.params) : m.texto
}

/** O texto no idioma que o `<html>` mostra agora (para quem só aceita texto); sem DOM, o padrão. */
export function textoAgora(m: Mensagem): string {
  const lang = globalThis.document?.documentElement?.lang
  return textoDaMensagem(tradutor(lang && isAppLanguage(lang) ? lang : FALLBACK_LANGUAGE), m)
}

/**
 * Um erro que já sabe a chave do catálogo: quem pode, guarda a chave e traduz ao mostrar; quem só lê
 * `message` recebe o texto no idioma da hora em que o erro nasceu.
 */
export class ErroComChave extends Error {
  readonly chave: ChaveTexto
  constructor(chave: ChaveTexto, options?: ErrorOptions) {
    super(textoAgora({ chave }), options)
    this.chave = chave
  }
}

/** A mensagem de um erro: a chave, quando ele a traz; senão o texto pronto de `fallback`. */
export function mensagemDoErro(error: unknown, fallback: (error: unknown) => string): Mensagem {
  return error instanceof ErroComChave ? { chave: error.chave } : { texto: fallback(error) }
}
