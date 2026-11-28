// Registro do IPC do ouvinte de retorno do login (fork-oauth-loopback.ts), sem depender da identidade do app:
// o contexto geral e a entrada Open passam a porta e a guarda do remetente.
import { ipcMain, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import { idiomaDoApp } from './fork-idioma'
import { RetornoDoLogin, type Resultado } from './fork-oauth-loopback'
import { openExternalURL } from './navigation'

export function registrarRetornoDoLogin(opcoes: {
  porta: number
  allowed: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean
  autorizar?: (url: string, nonce: string | null) => boolean
  aoFechar?: (nonce: string | null) => void
}) {
  const retorno = new RetornoDoLogin({ porta: opcoes.porta, abrir: openExternalURL, idioma: idiomaDoApp, autorizar: opcoes.autorizar, aoFechar: opcoes.aoFechar })
  ipcMain.handle('login-retorno:abrir', (e, nonce: unknown) => (opcoes.allowed(e) ? retorno.abrir(nonce) : null))
  ipcMain.handle('login-retorno:esperar', (e, tentativa: unknown, url: unknown) => {
    if (!opcoes.allowed(e)) return Promise.reject(new Error('Contexto indisponível.')) // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
    return retorno.esperar(tentativa, url)
  })
  ipcMain.on('login-retorno:responder', (e, tentativa: unknown, valor: unknown) => {
    if (!opcoes.allowed(e) || typeof valor !== 'object' || valor === null) return
    const { ok, nome, detalhe } = valor as Partial<Resultado>
    retorno.responder(tentativa, { ok: ok === true, ...(typeof nome === 'string' ? { nome } : {}), ...(typeof detalhe === 'string' ? { detalhe } : {}) })
  })
  ipcMain.on('login-retorno:fechar', (e, tentativa: unknown) => {
    if (opcoes.allowed(e)) retorno.encerrar(tentativa)
  })
  return retorno
}
