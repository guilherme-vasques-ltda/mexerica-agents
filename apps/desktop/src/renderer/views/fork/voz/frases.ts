// Fatia 6b (D454): as expressões que mudam com o aparelho ("neste Mac", "neste computador") e com o provedor
// ("da OpenAI", "do Gemini") saem do catálogo voz/ajustes, no idioma de quem desenha; as frases inteiras ficam no
// catálogo com `{neste}`, `{deste}`, `{aparelho}`, `{daProvedor}`, `{DaProvedor}` e `{naProvedor}`.
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { webDesteAparelho } from '../../../lib/atalhos.ts'

export type ProvedorVoz = 'openai' | 'gemini'

/** No Mac, "neste Mac"; fora dele, "neste computador". */
export const ehMac = () => globalThis.window?.codync?.platform === 'darwin'

/**
 * As expressões do aparelho ("neste Mac", "neste computador"). Na Web o aparelho é o navegador: é um dispositivo, nunca
 * um computador, e as frases dizem "neste dispositivo" (a decisão Web × app é a de lib/atalhos, a mesma do aviso de voz).
 */
export function frasesAparelho(t: Tradutor, mac = ehMac(), web = webDesteAparelho()) {
  if (web) return { neste: t('voz.ajustes.aparelho.dispositivo.neste'), deste: t('voz.ajustes.aparelho.dispositivo.deste'), aparelho: t('voz.ajustes.aparelho.dispositivo.nome') }
  const a = mac ? 'mac' : 'pc'
  return { neste: t(`voz.ajustes.aparelho.${a}.neste`), deste: t(`voz.ajustes.aparelho.${a}.deste`), aparelho: t(`voz.ajustes.aparelho.${a}.nome`) }
}

export function frasesProvedor(t: Tradutor, p: ProvedorVoz) {
  return { daProvedor: t(`voz.ajustes.provedor.${p}.da`), DaProvedor: t(`voz.ajustes.provedor.${p}.Da`), naProvedor: t(`voz.ajustes.provedor.${p}.na`) }
}

/** O texto de uma mensagem guardada como dado (D507), com as expressões do aparelho e do provedor do idioma de `t`. */
export function textoVoz(t: Tradutor, m: Mensagem, p?: ProvedorVoz, mac = ehMac()): string {
  if (!('chave' in m)) return m.texto
  const params = { ...frasesAparelho(t, mac), ...(p ? frasesProvedor(t, p) : {}), ...m.params }
  return (t as unknown as (c: string, p: Record<string, string | number>) => string)(m.chave, params)
}

/** Várias mensagens (frases inteiras e independentes), uma depois da outra. */
export const textosVoz = (t: Tradutor, ms: readonly Mensagem[], p?: ProvedorVoz, mac = ehMac()) => ms.map((m) => textoVoz(t, m, p, mac)).join(' ')
