/// <reference types="vite/client" />
import type { CodyncBridge } from '@shared/ipc'

declare global {
  interface Window {
    codync: CodyncBridge
    /** MX-209: só existe na janela do cartão seguro (preload/fork-cofre-seguro.ts). */
    mxCofre: import('@shared/fork-cofre-seguro').PonteCofre
  }
}

export {}
