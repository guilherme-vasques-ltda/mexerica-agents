import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/tema.ts'

// DON-903: tema (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'tema.grupo': 'المظهر',
  'tema.claro': 'المظهر الفاتح',
  'tema.escuro': 'المظهر الداكن',
}
