import { createContext } from 'react'

/** Abre Ajustes → Computadores desta janela; ausente, a linha de "sem tela" fica sem link. */
export const AbrirComputadores = createContext<(() => void) | null>(null)
