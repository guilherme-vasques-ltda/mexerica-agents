import type { CodyncBridge } from './ipc.ts'

export interface Capacidades {
  launchAtLogin: boolean
  appIcon: boolean
  ssh: boolean
  cofreLocal: boolean
  perfilConta: boolean
}

type BridgeCapacidades = Pick<CodyncBridge, 'web'> & { platform?: string }

/** Sem ponte ou plataforma reconhecida, os recursos nativos ficam desabilitados. */
export function capacidades(bridge?: BridgeCapacidades | null): Capacidades {
  const desktop = bridge?.web !== true && (
    bridge?.platform === 'darwin' || bridge?.platform === 'linux' || bridge?.platform === 'win32'
  )
  return {
    launchAtLogin: desktop,
    appIcon: desktop,
    ssh: desktop,
    cofreLocal: desktop,
    perfilConta: desktop,
  }
}
