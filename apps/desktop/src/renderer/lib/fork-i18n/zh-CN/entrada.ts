import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/entrada.ts'

// DON-903: entrada (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'entrada.boasVindas': '欢迎使用 Mexerica',
  'entrada.marca': 'Mexerica',
  'entrada.restaurando': '正在检查已保存的账户…',
}
