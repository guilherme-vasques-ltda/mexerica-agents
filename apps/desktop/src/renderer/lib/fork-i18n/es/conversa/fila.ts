import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/fila.ts'

// DON-903: conversa/fila (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 (D431): a bandeja da fila em cima do campo (views/fork/conversa/BandejaFila.tsx).
  'conversa.fila.titulo': 'En cola',
  'conversa.fila.lista': 'Mensajes en cola',
  'conversa.fila.respondendo': '{nome} está respondiendo…',
  'conversa.fila.atalhos': '⇧Enter envía el 1.º · ↑ edita el último',
  'conversa.fila.recolher': 'Contraer la cola',
  'conversa.fila.expandir': 'Mostrar la cola',
  'conversa.fila.mandarAgora': 'Enviar ahora',
  'conversa.fila.editar': 'Editar',
  'conversa.fila.remover': 'Quitar',
  'conversa.fila.arrastar': 'Arrastra para cambiar el orden (Alt+↑/↓)',
  'conversa.fila.soAnexos': 'Solo adjuntos',
  'conversa.fila.de': 'De {origem}',
  'conversa.fila.editado': 'editado',
  'conversa.fila.editando': 'Editando el n.º {n} de la cola',
  'conversa.fila.cancelar': 'Cancelar',
  'conversa.fila.salvar': 'Guardar',
  'conversa.fila.erro.saiu': 'Ese mensaje ya salió de la cola.',
  'conversa.fila.erro.mudou': 'La cola cambió mientras tanto. Revísala e inténtalo de nuevo.',
  'conversa.fila.erro.permissao': 'Este dispositivo ya no puede cambiar la cola.',
  'conversa.fila.erro.pendente': 'El ordenador no confirmó a tiempo. Revisa la cola.',
  'conversa.fila.erro.falhou': 'No se pudo cambiar la cola.',
  'conversa.fila.erro.cheia': 'La cola de esta conversación está llena. Espera a que salgan algunos mensajes y vuelve a enviar.',
}
