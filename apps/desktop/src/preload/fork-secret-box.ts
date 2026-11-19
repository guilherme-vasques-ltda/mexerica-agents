import { contextBridge, ipcRenderer } from 'electron'

// Preload próprio da caixa D277 (não é o preload geral): uma única ação, o gesto do dono sobre um
// pedido mostrado. O nonce da instância vem do main por argumento e não fica visível à página.
const prefix = '--donna-box-nonce='
const nonce = process.argv.find(arg => arg.startsWith(prefix))?.slice(prefix.length) ?? ''

contextBridge.exposeInMainWorld('donnaBox', {
  gesture: (requestRef: string) => ipcRenderer.invoke('fork-secret-box:gesture', nonce, String(requestRef)),
})
