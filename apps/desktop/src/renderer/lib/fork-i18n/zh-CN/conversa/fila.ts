import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/fila.ts'

// DON-903: conversa/fila (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 (D431): a bandeja da fila em cima do campo (views/fork/conversa/BandejaFila.tsx).
  'conversa.fila.titulo': '排队中',
  'conversa.fila.lista': '排队的消息',
  'conversa.fila.respondendo': '{nome} 正在回复…',
  'conversa.fila.atalhos': '⇧Enter 发送第 1 条 · ↑ 编辑最后一条',
  'conversa.fila.recolher': '收起队列',
  'conversa.fila.expandir': '显示队列',
  'conversa.fila.mandarAgora': '立即发送',
  'conversa.fila.editar': '编辑',
  'conversa.fila.remover': '移除',
  'conversa.fila.arrastar': '拖动以调整顺序（Alt+↑/↓）',
  'conversa.fila.soAnexos': '仅附件',
  'conversa.fila.de': '来自 {origem}',
  'conversa.fila.editado': '已编辑',
  'conversa.fila.editando': '正在编辑队列中的第 {n} 条',
  'conversa.fila.cancelar': '取消',
  'conversa.fila.salvar': '保存',
  'conversa.fila.erro.saiu': '这条消息已经离开队列。',
  'conversa.fila.erro.mudou': '队列已变化，请检查后重试。',
  'conversa.fila.erro.permissao': '此设备已不能再修改队列。',
  'conversa.fila.erro.pendente': '电脑未及时确认，请检查队列。',
  'conversa.fila.erro.falhou': '无法修改队列。',
  'conversa.fila.erro.cheia': '此对话的队列已满。请等几条消息发出后再发送。',
}
