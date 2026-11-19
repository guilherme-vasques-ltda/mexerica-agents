// Auditoria D249 (E2): a ilha numa parte própria do pacote, sem a conversa, o terminal, Times, Rotinas nem Ajustes.
// O processo principal abre a mesma página (`?window=island`); só o JS que ela pede muda.
import type { Root } from 'react-dom/client'
import { IslandWindow } from '../../island/IslandWindow'
import { IslandModel } from '../../island/island-model'

export function abrir(root: Root) {
  // The island only draws what the chat window sends it: no host connection of its own.
  const island = new IslandModel()
  island.start()
  // MX-245, só a prévia do site (`donnabotEditorDemo` já marca a demo): abre direto na aba Uso, para a
  // captura do cartão sem precisar simular o hover e o clique reais.
  if (Object.hasOwn(window, 'donnabotEditorDemo')) {
    Object.defineProperty(window, 'donnabotIslandDemo', { value: { abrirUso: () => { island.openNow(); island.setTab('usage') } } })
  }
  root.render(<IslandWindow model={island} />)
}
