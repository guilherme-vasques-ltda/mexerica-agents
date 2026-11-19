import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/troca.ts'

// DON-903: conversa/troca (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 C9a: a linha de troca de modelo e o balão técnico (views/fork/troca-modelo).
  'conversa.troca.linha': '模型已从 {de} 换为 {para}。对话已转交给新模型。',
  'conversa.troca.balao.sessao': '新模型接续了整个对话。',
  'conversa.troca.balao.passagem': '新模型收到了摘要和最近的请求，而不是整个对话。',
  'conversa.troca.balao.enviando': '正在发送…',
  'conversa.troca.balao.reenviados': '重新发送了 {n} 个 token，没有利用已准备好的内容。',
  'conversa.troca.balao.enviados': '发送了 {n} 个 token，没有利用已准备好的内容。',
}
