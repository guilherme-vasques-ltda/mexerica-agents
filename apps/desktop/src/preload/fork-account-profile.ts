import { ipcRenderer } from 'electron'
import type { AccountProfileBridge } from '../shared/fork-account-profile'

/** E6b: ponte estreita de Ajustes → Conta. Canais fixos; o Main confere a janela e a sessão. */
export const accountProfileBridge: AccountProfileBridge = {
  load: () => ipcRenderer.invoke('accountProfile:load'),
  saveName: (first, last) => ipcRenderer.invoke('accountProfile:saveName', first, last),
  setPassword: (current, next) => ipcRenderer.invoke('accountProfile:setPassword', current, next),
  link: (provider) => ipcRenderer.invoke('accountProfile:link', provider),
  unlink: (id) => ipcRenderer.invoke('accountProfile:unlink', id),
  startTotp: () => ipcRenderer.invoke('accountProfile:startTotp'),
  confirmTotp: (code) => ipcRenderer.invoke('accountProfile:confirmTotp', code),
  disableTotp: () => ipcRenderer.invoke('accountProfile:disableTotp'),
  regenerateCodes: () => ipcRenderer.invoke('accountProfile:regenerateCodes'),
  saveCodes: (codes) => ipcRenderer.invoke('accountProfile:saveCodes', codes),
  startReverify: () => ipcRenderer.invoke('accountProfile:startReverify'),
  sendReverifyCode: () => ipcRenderer.invoke('accountProfile:sendReverifyCode'),
  attemptReverify: (attempt) => ipcRenderer.invoke('accountProfile:attemptReverify', attempt),
}
