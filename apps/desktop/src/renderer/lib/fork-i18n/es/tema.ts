import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/tema.ts'

// DON-903: tema (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'tema.grupo': 'Tema',
  'tema.claro': 'Tema claro',
  'tema.escuro': 'Tema oscuro',
}
