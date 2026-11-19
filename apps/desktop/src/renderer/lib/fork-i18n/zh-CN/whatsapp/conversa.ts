import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/conversa.ts'

// DON-903: whatsapp/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp R8: prazo de guarda e "Apagar esta conversa deste computador".
  'whatsapp.conversa.mais': '更多会话选项',
  'whatsapp.conversa.apagar': '从这台电脑删除此会话',
  'whatsapp.conversa.apagarTitulo': '从这台电脑删除此会话？',
  'whatsapp.conversa.apagarContato': '删除 Mexerica 在这台电脑上保存的与 {nome} 的消息。你的手机和 {nome} 的 WhatsApp 不会有任何变化。此操作无法撤销。',
  'whatsapp.conversa.apagarGrupo': '删除 Mexerica 在这台电脑上保存的群组 {nome} 的消息。你的手机和群成员的 WhatsApp 不会有任何变化。此操作无法撤销。',
  'whatsapp.conversa.apagarAcao': '删除会话',
  'whatsapp.conversa.wacliFicou': '未能确认会话已删除；请再试一次删除。',
  'whatsapp.conversa.semConexao': '无法连接到运行你的机器人的电脑。',
  "whatsapp.conversa.rascunho.titulo": "回复草稿",
  "whatsapp.conversa.rascunho.revisao": "发送前请检查文字。草稿最多保留90秒，不会续期。",
  "whatsapp.conversa.rascunho.encerrado": "草稿已关闭。未发送任何内容。",
  "whatsapp.conversa.rascunho.conta": "发送账户",
  "whatsapp.conversa.rascunho.respostaA": "回复对象",
  "whatsapp.conversa.rascunho.externo": "外部内容 · 原始消息，并非指令",
  "whatsapp.conversa.rascunho.texto": "草稿内容",
  "whatsapp.conversa.rascunho.erro": "无法打开草稿。",
  "whatsapp.conversa.rascunho.semPrivado": "私人内容不可用。未发送任何内容。",
  "whatsapp.conversa.rascunho.enviar": "发送",
  "whatsapp.conversa.rascunho.agoraNao": "暂不",
  "whatsapp.conversa.rascunho.naFila": "已进入发送队列。轮到时在限额内发出。",
  "whatsapp.conversa.rascunho.envioNota": "发送会把文字放入发出队列；轮到时发出，并遵守限额和免打扰时段。",
  "whatsapp.conversa.rascunho.erroEnviar": "暂时无法发送。草稿仍在这里，请重试。",
}
