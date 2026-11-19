// Módulo pequeno e sem catálogos: quem só precisa reconhecer (`instanceof`) ou lançar um `ErroFrase` não deve puxar
// os catálogos de texto (~4,5 MB) para o seu grafo de módulos. O texto em inglês da mensagem crua é preenchido por
// `frase.ts` (que tem o catálogo) ao ser carregado; sem ele a mensagem crua fica vazia e a `frase` segue intacta.
import type { Frase } from './frase.ts'

let textoCru: ((frase: Frase) => string) | null = null

/** Chamado só por `frase.ts`: liga a mensagem crua em inglês ao catálogo. */
export function registrarTextoCru(resolver: (frase: Frase) => string) {
  textoCru = resolver
}

/** Um erro que a própria interface lança: leva a frase, traduzida só ao mostrar; a mensagem crua fica em inglês. */
export class ErroFrase extends Error {
  readonly frase: Frase
  constructor(frase: Frase) {
    super(textoCru?.(frase))
    this.frase = frase
  }
}
