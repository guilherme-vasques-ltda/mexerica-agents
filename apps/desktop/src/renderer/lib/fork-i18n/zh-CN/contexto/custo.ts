import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/contexto/custo.ts'

// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-908 C8
  'contexto.custo.titulo': '此对话的预估费用',
  'contexto.custo.parcial': '部分：{total} 轮中有 {n} 轮无价格',
  'contexto.custo.vazio': '自 Hub 更新以来，此对话尚无记录的轮次。',
  'contexto.custo.semTokens': '无 token 记录',
  'contexto.custo.falha': '暂时无法获取费用。',
  'contexto.custo.nota': '按 API 等价估算。订阅时不按 token 付费；真正有限制的是账户额度。',
  'contexto.custo.parte.entrada': '输入 {n}',
  'contexto.custo.parte.cacheLido': '缓存读取 {n}',
  'contexto.custo.parte.cacheGravado': '缓存写入 {n}',
  'contexto.custo.parte.saida': '输出 {n}',
}
