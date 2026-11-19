import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/fila.ts'

// DON-903: conversa/fila (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 (D431): a bandeja da fila em cima do campo (views/fork/conversa/BandejaFila.tsx).
  'conversa.fila.titulo': 'En attente',
  'conversa.fila.lista': 'Messages en attente',
  'conversa.fila.respondendo': '{nome} répond…',
  'conversa.fila.atalhos': '⇧Entrée envoie le 1er · ↑ modifie le dernier',
  'conversa.fila.recolher': 'Replier la file',
  'conversa.fila.expandir': 'Afficher la file',
  'conversa.fila.mandarAgora': 'Envoyer maintenant',
  'conversa.fila.editar': 'Modifier',
  'conversa.fila.remover': 'Retirer',
  'conversa.fila.arrastar': "Glisser pour changer l'ordre (Alt+↑/↓)",
  'conversa.fila.soAnexos': 'Pièces jointes seules',
  'conversa.fila.de': 'De {origem}',
  'conversa.fila.editado': 'modifié',
  'conversa.fila.editando': 'Modification du n° {n} de la file',
  'conversa.fila.cancelar': 'Annuler',
  'conversa.fila.salvar': 'Enregistrer',
  'conversa.fila.erro.saiu': 'Ce message a déjà quitté la file.',
  'conversa.fila.erro.mudou': 'La file a changé entre-temps. Vérifiez et réessayez.',
  'conversa.fila.erro.permissao': 'Cet appareil ne peut plus modifier la file.',
  'conversa.fila.erro.pendente': "L'ordinateur n'a pas confirmé à temps. Vérifiez la file.",
  'conversa.fila.erro.falhou': 'Impossible de modifier la file.',
}
