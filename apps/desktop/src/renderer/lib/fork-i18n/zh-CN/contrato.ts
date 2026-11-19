import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/contrato.ts'

// DON-903: contrato (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'contrato.titulo': "附带约定",
  'contrato.resultado': "预期结果",
  'contrato.aceite': "如何验收",
  'contrato.escopo': "可操作范围",
  'contrato.limites': "限制",
  'contrato.regra': "遵循的委派规则",
  'contrato.nota': "由发起请求的机器人提出。它只指导交付，不授予权限，也不改变限制。",
}
