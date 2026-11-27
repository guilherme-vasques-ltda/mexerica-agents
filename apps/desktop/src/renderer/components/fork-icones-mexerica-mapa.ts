/**
 * Biblioteca de ícones de interface da Mexerica (brand/assets/icon/mexerica): o manifesto diz que arquivo desenha cada
 * ícone e que símbolos do app ele cobre. Daqui sai um mapa nome → miolo do SVG, aceitando o `id` do manifesto
 * (`mx-enviar`) ou um símbolo de `simbolosDoApp` (`mic`). O que não está no manifesto devolve null e o Icon segue
 * como antes.
 */

export interface IconeDoManifesto {
  id: string
  arquivo: string
  simbolosDoApp: string[]
}

export interface Manifesto {
  icones: IconeDoManifesto[]
}

/** A raiz que todo SVG do pacote usa; o Icon desenha a própria raiz com estes atributos e injeta só o miolo. */
export const RAIZ_DO_PACOTE =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">'

/**
 * O miolo do SVG, ou null quando a raiz não é a do pacote ou o miolo traz algo além de `<path d="…"/>` (a folha da
 * assinatura é um `path` cheio, também em currentColor).
 */
export function mioloDoSvg(svg: string): string | null {
  const texto = svg.trim()
  if (!texto.startsWith(RAIZ_DO_PACOTE) || !texto.endsWith('</svg>')) return null
  const miolo = texto.slice(RAIZ_DO_PACOTE.length, -'</svg>'.length)
  return /^(<path d="[MmLlHhVvCcSsQqTtAaZz0-9 .,-]+"( fill="currentColor" stroke="none")?\/>)+$/.test(miolo) ? miolo : null
}

/** `arquivos` vai do caminho relativo do manifesto (`svg/mx-enviar.svg`) ao texto do SVG. */
export function montarIcones(manifesto: Manifesto, arquivos: ReadonlyMap<string, string>): Map<string, string> {
  const mapa = new Map<string, string>()
  for (const icone of manifesto.icones) {
    const svg = arquivos.get(icone.arquivo)
    const miolo = svg === undefined ? null : mioloDoSvg(svg)
    if (miolo === null) continue
    mapa.set(icone.id, miolo)
    for (const simbolo of icone.simbolosDoApp) mapa.set(simbolo, miolo)
  }
  return mapa
}
