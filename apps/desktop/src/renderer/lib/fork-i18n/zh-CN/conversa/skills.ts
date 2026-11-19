import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/skills.ts'

// Fatia 2 (D454): conversa/skills (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "conversa.skills.motivo": "原因",
  "conversa.skills.agoraNao": "暂不",
  "conversa.skills.examinarRascunho": "查看草稿",
  "conversa.skills.soPessoal": "技能只在“个人”中可用。此 Space 无法打开或保存到个人目录。",
  "conversa.skills.indicacao.aria": "技能推荐线索",
  "conversa.skills.indicacao.de": "来自 {bot} 的推荐线索",
  "conversa.skills.indicacao.referencia": "建议的参考",
  "conversa.skills.indicacao.pista": "这是按名称或 ID 搜索的线索，不代表该技能存在、版本、已安装或被机器人使用。查看只会打开一次搜索。",
  "conversa.skills.indicacao.soPessoal": "技能只在“个人”中可用。此 Space 无法查询个人目录。",
  "conversa.skills.indicacao.examinar": "查看推荐线索",
  "conversa.skills.recomendacao.aria": "技能推荐",
  "conversa.skills.recomendacao.de": "来自 {bot} 的推荐",
  "conversa.skills.recomendacao.quando": "何时使用",
  "conversa.skills.recomendacao.instrucoes": "建议的指令",
  "conversa.skills.recomendacao.nada": "提出并打开此草稿不会保存、启用技能或批准操作。",
  "conversa.skills.rascunho.revise": "文本草稿。保存前请检查；不会启用任何机器人。",
}
