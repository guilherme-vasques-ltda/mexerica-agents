// IPC do ouvinte de retorno do login de conector (fork-oauth-loopback.ts). Só o contexto geral do app o usa.
import { ipcMain } from 'electron'
import { identity } from './environment'
import { idiomaDoApp } from './fork-idioma'
import { PORTA_DEV, PORTA_PRINCIPAL, RetornoDoLogin, type Resultado } from './fork-oauth-loopback'
import { isGeneralContext } from './fork-secret-box/contexts'
import { openExternalURL } from './navigation'

export function registerOAuthLoopbackIPC() {
  const retorno = new RetornoDoLogin({
    porta: identity.environment === 'dev' ? PORTA_DEV : PORTA_PRINCIPAL,
    abrir: openExternalURL,
    idioma: idiomaDoApp,
  })
  ipcMain.handle('login-retorno:abrir', (e) => (isGeneralContext(e.sender) ? retorno.abrir() : null))
  ipcMain.handle('login-retorno:esperar', (e, tentativa: unknown, url: unknown) => {
    if (!isGeneralContext(e.sender)) return Promise.reject(new Error('Contexto indisponível.')) // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
    return retorno.esperar(tentativa, url)
  })
  ipcMain.on('login-retorno:responder', (e, tentativa: unknown, valor: unknown) => {
    if (!isGeneralContext(e.sender) || typeof valor !== 'object' || valor === null) return
    const { ok, nome, detalhe } = valor as Partial<Resultado>
    retorno.responder(tentativa, { ok: ok === true, ...(typeof nome === 'string' ? { nome } : {}), ...(typeof detalhe === 'string' ? { detalhe } : {}) })
  })
  ipcMain.on('login-retorno:fechar', (e, tentativa: unknown) => {
    if (isGeneralContext(e.sender)) retorno.encerrar(tentativa)
  })
}
