import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/memoria/sugestoes.ts'

// DON-903: memoria/sugestoes (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // D389: Sugestões na Memória do bot (views/fork/memoria-auto) e o contador do cartão.
  'memoria.sugestoes.esperando': "待处理的建议：{n}",
  'memoria.sugestoes.aba': "建议",
  'memoria.sugestoes.lead': "机器人想要保存的内容。在你批准之前，这些都不会进入记忆。",
  'memoria.sugestoes.vazio': "没有待处理的建议。",
  'memoria.sugestoes.aprovar': "批准",
  'memoria.sugestoes.descartar': "放弃",
  'memoria.sugestoes.acao.remember': "新记忆",
  'memoria.sugestoes.acao.revise': "修改一条记忆",
  'memoria.sugestoes.acao.archive': "归档一条记忆",
  'memoria.sugestoes.acao.confirm': "确认：这是你说的",
  'memoria.sugestoes.acao.reclassify': "根据你的消息确认",
  'memoria.sugestoes.acao.note': "笔记 {arquivo}",
  'memoria.sugestoes.removerNota': "删除此笔记",
  'memoria.sugestoes.compartilhada': "适用于你所有的机器人",
  'memoria.sugestoes.mudou': "此建议之后记忆已更改，未覆盖任何内容。",
  'memoria.sugestoes.semMudanca': "记忆中已有此内容。",
  'memoria.sugestoes.erro': "无法完成，请重试。",
}
