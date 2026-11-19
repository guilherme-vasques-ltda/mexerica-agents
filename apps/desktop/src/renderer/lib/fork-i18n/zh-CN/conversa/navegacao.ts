import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/navegacao.ts'

// D432: conversa/navegacao (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // Barrinha, linha "Novas" e "Ir para o fim" (docs/reference/design/barrinha-novas-20261107).
  'conversa.navegacao.rotulo': "对话时间线",
  'conversa.navegacao.ir': "跳到 {hora}：{texto}",
  'conversa.navegacao.irGrupo': "跳到 {hora} · {n} 条消息：{texto}",
  'conversa.navegacao.semTexto': "消息",
  'conversa.navegacao.novas': "新消息",
  'conversa.navegacao.novasRotulo': "以下是新消息",
  'conversa.navegacao.fim': "跳到最后",
  'conversa.navegacao.fimNovas.other': "{n} 条新消息",
}
