// Auditoria D249 (E2): a janela de conversa e a de pareamento, numa parte do pacote que a ilha não carrega.
import type { Root } from 'react-dom/client'
import { useLayoutEffect, type ReactNode } from 'react'
import { ChatWindow } from '../../ChatWindow'
import { PairingWindow } from '../../PairingWindow'
import { AppModel } from '../../../store/app-model'
import { AppContext } from '../../../store/context'
import { appearanceReady } from '../../../lib/appearance'
import { account } from '../../../store/account'
import { primeiraTelaMontada } from '../../../lib/abertura'

/** MX-93: no commit da primeira tela, a abertura do index.html sai (e fica a marca de tempo da MX-94). */
function PrimeiraTela({ children }: { children: ReactNode }) {
  useLayoutEffect(primeiraTelaMontada, [])
  return children
}

export function abrir(root: Root, page: string) {
  // The pairing window only needs this computer's host, not a mirror of its bots.
  const app = new AppModel(page !== 'pairing')
  void app.start()
  if (page !== 'pairing') void account.start()
  // aparencia-01: with a picture of their own, the first frame waits for it (at most 300 ms).
  void appearanceReady().then(() => root.render(<PrimeiraTela><AppContext.Provider value={app}>{page === 'pairing' ? <PairingWindow /> : <ChatWindow />}</AppContext.Provider></PrimeiraTela>))
}
