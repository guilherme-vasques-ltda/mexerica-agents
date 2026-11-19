import { contextBridge, ipcRenderer } from 'electron'
import type { PonteCofre } from '../shared/fork-cofre-seguro'

// MX-209: preload próprio da janela do cartão seguro (não é o preload geral; aqui não existe `window.codync`).
// Três ações e nada mais. O nonce da abertura vem do main por argumento e não fica visível à página.
// Canais escritos aqui de propósito, iguais aos de `cofreCanais`: importar o módulo faria o rollup criar
// um chunk compartilhado e preload com sandbox não consegue `require` de outro arquivo.
const dados = 'fork-cofre-seguro:dados'
const salvar = 'fork-cofre-seguro:salvar'
const cancelar = 'fork-cofre-seguro:cancelar'

const prefixo = '--mx-cofre-nonce='
const nonce = process.argv.find((arg) => arg.startsWith(prefixo))?.slice(prefixo.length) ?? ''

const ponte: PonteCofre = {
  dados: () => ipcRenderer.invoke(dados, nonce),
  salvar: (envio) => ipcRenderer.invoke(salvar, nonce, { valor: String(envio.valor), usuario: String(envio.usuario ?? ''), rotulo: String(envio.rotulo ?? ''), ...(envio.site === undefined ? {} : { site: String(envio.site) }) }),
  cancelar: () => ipcRenderer.send(cancelar, nonce),
}
contextBridge.exposeInMainWorld('mxCofre', ponte)
