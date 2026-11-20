import type { CodyncBridge } from './ipc.ts'

export interface Capacidades {
  dock: boolean
  launchAtLogin: boolean
  appIcon: boolean
  ssh: boolean
  cofreLocal: boolean
  perfilConta: boolean
}

type BridgeCapacidades = Pick<CodyncBridge, 'web'> & { platform?: string; appScheme?: string }

/** Sem ponte ou plataforma reconhecida, os recursos nativos ficam desabilitados. */
export function capacidades(bridge?: BridgeCapacidades | null): Capacidades {
  const desktop = bridge?.web !== true && (
    bridge?.platform === 'darwin' || bridge?.platform === 'linux' || bridge?.platform === 'win32'
  )
  const preferenciasNativas = desktop && bridge?.appScheme !== 'mexerica-open'
  return {
    dock: preferenciasNativas && bridge?.platform === 'darwin',
    launchAtLogin: preferenciasNativas,
    appIcon: preferenciasNativas,
    ssh: desktop,
    cofreLocal: desktop,
    perfilConta: desktop,
  }
}
