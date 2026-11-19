import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/fila.ts'

// DON-903: conversa/fila (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 (D431): a bandeja da fila em cima do campo (views/fork/conversa/BandejaFila.tsx).
  'conversa.fila.titulo': 'Queued',
  'conversa.fila.lista': 'Queued messages',
  'conversa.fila.respondendo': '{nome} is replying…',
  'conversa.fila.atalhos': '⇧Enter sends the 1st · ↑ edits the last',
  'conversa.fila.recolher': 'Collapse the queue',
  'conversa.fila.expandir': 'Show the queue',
  'conversa.fila.mandarAgora': 'Send now',
  'conversa.fila.editar': 'Edit',
  'conversa.fila.remover': 'Remove',
  'conversa.fila.arrastar': 'Drag to reorder (Alt+↑/↓)',
  'conversa.fila.soAnexos': 'Attachments only',
  'conversa.fila.de': 'From {origem}',
  'conversa.fila.editado': 'edited',
  'conversa.fila.editando': 'Editing #{n} in the queue',
  'conversa.fila.cancelar': 'Cancel',
  'conversa.fila.salvar': 'Save',
  'conversa.fila.erro.saiu': 'That message already left the queue.',
  'conversa.fila.erro.mudou': 'The queue changed meanwhile. Check it and try again.',
  'conversa.fila.erro.permissao': 'This device can no longer change the queue.',
  'conversa.fila.erro.pendente': "The computer didn't confirm in time. Check the queue.",
  'conversa.fila.erro.falhou': "Couldn't change the queue.",
}
