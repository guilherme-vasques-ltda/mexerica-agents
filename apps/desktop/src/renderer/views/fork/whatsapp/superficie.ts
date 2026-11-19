import { createContext } from 'react'

// D387: a superfície (a conversa do bot no centro) está à vista. Com a tela grande do WhatsApp aberta, a conversa fica
// montada e só é escondida por CSS (`.conversation.wa-x`), e um número revelado nela (ex.: conta de saída num cartão)
// não pode ficar no DOM oculto. A ThreadView liga; fora dela, à vista.
export const SuperficieAVista = createContext(true)
