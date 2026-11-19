import { ipcRenderer } from 'electron'
import type { VozBridge } from '../shared/fork-voz'

/** voz-04: ponte estreita da chave deste aparelho. Canais fixos, sem `invoke` exposto, e nenhum que leia a chave. */
export const vozBridge: VozBridge = {
  catalogoXai: () => ipcRenderer.invoke('voz:xai:catalogo'),
  credencialXai: (pedido) => ipcRenderer.invoke('voz:xai:credencial', pedido),
  conferirSessaoXai: (id, sessao) => ipcRenderer.invoke('voz:xai:conferir-sessao', id, sessao),
  fecharSessaoXai: (id) => ipcRenderer.invoke('voz:xai:fechar-sessao', id),
  estado: () => ipcRenderer.invoke('voz:estado'),
  guardar: (provider, chave) => ipcRenderer.invoke('voz:guardar', provider, chave),
  limpar: (provider) => ipcRenderer.invoke('voz:limpar', provider),
  testar: (provider) => ipcRenderer.invoke('voz:testar', provider),
  usarReserva: (provider) => ipcRenderer.invoke('voz:usarReserva', provider),
  credencial: (provider, model, voice) => ipcRenderer.invoke('voz:credencial', provider, model, voice),
  onChange: (listener) => {
    // Só "leia de novo": nada do evento chega ao renderer.
    const handler = () => listener()
    ipcRenderer.on('voz:changed', handler)
    return () => void ipcRenderer.removeListener('voz:changed', handler)
  },
}
