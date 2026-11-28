import manifesto from '../../../../../brand/assets/icon/mexerica/manifesto.json'
import { montarIcones } from './fork-icones-mexerica-mapa'

/**
 * Os SVGs do pacote, pelo glob do Vite (trocado por imports na build). Testes que montam telas numa VM sem o Vite não
 * têm o glob: lá o mapa fica vazio e o Icon desenha como antes.
 */
function desenhos(): Record<string, string> {
  try {
    return import.meta.glob<string>('../../../../../brand/assets/icon/mexerica/svg/*.svg', { eager: true, query: '?raw', import: 'default' })
  } catch {
    return {}
  }
}

let icones: Map<string, string> | null = null

/** O miolo do ícone da Mexerica para um `id` do manifesto ou um símbolo que ele cobre; null fora do manifesto. */
export function iconeMexerica(nome: string): string | null {
  icones ??= montarIcones(manifesto, new Map(Object.entries(desenhos()).map(([caminho, svg]) => [caminho.slice(caminho.lastIndexOf('/svg/') + 1), svg])))
  return icones.get(nome) ?? null
}
