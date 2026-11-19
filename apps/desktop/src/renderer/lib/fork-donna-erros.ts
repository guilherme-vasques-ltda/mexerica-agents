// D366/D368: códigos estáveis que o Hub devolve quando recusa uma mudança no Berga fixo. A view
// mostra o texto do catálogo (botSettings.donnaFixa.erro.*), não o texto técnico do Hub.
import { FALLBACK_LANGUAGE, isAppLanguage } from '../../shared/fork-idioma.ts'
import { traduzir } from './fork-i18n/index.ts'

const CODIGOS = [
  ['donna_fixed', 'botSettings.donnaFixa.erro.fixa'],
  ['donna_reserved', 'botSettings.donnaFixa.erro.reservado'],
] as const

/** O idioma que `fork-i18n/estado` aplica ao `<html>`; sem DOM (testes), o padrão (inglês, D454/D495). */
function idioma() {
  const lang = globalThis.document?.documentElement?.lang
  return lang && isAppLanguage(lang) ? lang : FALLBACK_LANGUAGE
}

/** A chave do catálogo para uma recusa do Berga fixo, ou `null` (o texto sai na renderização, R1 do #572). */
export function chaveDoErroDaDonna(message: string) {
  return CODIGOS.find(([prefixo]) => message.startsWith(`${prefixo}:`))?.[1] ?? null
}

/** O texto do catálogo para uma recusa do Berga fixo, ou `null` se a mensagem não é uma delas. */
export function erroDaDonna(message: string): string | null {
  const codigo = CODIGOS.find(([prefixo]) => message.startsWith(`${prefixo}:`))
  return codigo ? traduzir(idioma(), codigo[1]) : null
}
