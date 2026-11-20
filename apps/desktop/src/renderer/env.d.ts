// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
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
