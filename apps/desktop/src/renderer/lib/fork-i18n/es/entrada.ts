import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/entrada.ts'

// DON-903: entrada (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'entrada.boasVindas': 'Te damos la bienvenida a Mexerica',
  'entrada.marca': 'Mexerica',
  'entrada.restaurando': 'Comprobando la cuenta guardada…',
}
