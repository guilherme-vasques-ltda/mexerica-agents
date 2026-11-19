import { test } from 'node:test'
import assert from 'node:assert/strict'
import { atalhoAria, atalhoDoApp, comAtalho, macNoAparelho, teclaDeBusca } from './atalhos.ts'
import { textos as ajustesPt } from './fork-i18n/pt-BR/ajustes.ts'
import { textos as ajustesEn } from './fork-i18n/en/ajustes.ts'
import { textos as ajustesFr } from './fork-i18n/fr/ajustes.ts'
import { textos as ajustesAr } from './fork-i18n/ar-AE/ajustes.ts'
import { textos as ajustesZh } from './fork-i18n/zh-CN/ajustes.ts'
import { textos as ajustesEs } from './fork-i18n/es/ajustes.ts'
import { textos as janelaPt } from './fork-i18n/pt-BR/navegacao/janela.ts'
import { textos as janelaEn } from './fork-i18n/en/navegacao/janela.ts'
import { textos as janelaFr } from './fork-i18n/fr/navegacao/janela.ts'
import { textos as janelaAr } from './fork-i18n/ar-AE/navegacao/janela.ts'
import { textos as janelaZh } from './fork-i18n/zh-CN/navegacao/janela.ts'
import { textos as janelaEs } from './fork-i18n/es/navegacao/janela.ts'

// Quatro aparelhos: Mac no app (Electron), Mac na Web, Windows/Linux no app e Web fora do Mac.
const MAC_APP = { mac: true, web: false }
const MAC_WEB = { mac: true, web: true }
const APP_FORA = { mac: false, web: false }
const WEB_FORA = { mac: false, web: true }

test('a busca tem ⌘K no Mac e Ctrl+K fora dele, no app e na Web', () => {
  assert.equal(atalhoDoApp('busca', MAC_APP.mac, MAC_APP.web), '⌘K')
  assert.equal(atalhoDoApp('busca', MAC_WEB.mac, MAC_WEB.web), '⌘K')
  assert.equal(atalhoDoApp('busca', APP_FORA.mac, APP_FORA.web), 'Ctrl+K')
  assert.equal(atalhoDoApp('busca', WEB_FORA.mac, WEB_FORA.web), 'Ctrl+K')
})

test('a barra lateral tem ⌃⌘S só no app do Mac: na Web em Mac não há tecla, então o rótulo não cita', () => {
  assert.equal(atalhoDoApp('barraLateral', MAC_APP.mac, MAC_APP.web), '⌃⌘S')
  assert.equal(atalhoDoApp('barraLateral', MAC_WEB.mac, MAC_WEB.web), null, 'Web em Mac: sem tecla')
  assert.equal(atalhoDoApp('barraLateral', APP_FORA.mac, APP_FORA.web), null, 'app fora do Mac')
  assert.equal(atalhoDoApp('barraLateral', WEB_FORA.mac, WEB_FORA.web), null, 'Web fora do Mac')
})

test('o aria-keyshortcuts da barra lateral é Control+Meta+S só no app do Mac; some nos outros três', () => {
  assert.equal(atalhoAria('barraLateral', MAC_APP.mac, MAC_APP.web), 'Control+Meta+S')
  assert.equal(atalhoAria('barraLateral', MAC_WEB.mac, MAC_WEB.web), null)
  assert.equal(atalhoAria('barraLateral', APP_FORA.mac, APP_FORA.web), null)
  assert.equal(atalhoAria('barraLateral', WEB_FORA.mac, WEB_FORA.web), null)
})

test('o aria-keyshortcuts da busca acompanha a plataforma: Meta+K no Mac, Control+K fora dele', () => {
  assert.equal(atalhoAria('busca', MAC_APP.mac, MAC_APP.web), 'Meta+K')
  assert.equal(atalhoAria('busca', MAC_WEB.mac, MAC_WEB.web), 'Meta+K')
  assert.equal(atalhoAria('busca', APP_FORA.mac, APP_FORA.web), 'Control+K')
  assert.equal(atalhoAria('busca', WEB_FORA.mac, WEB_FORA.web), 'Control+K')
})

test('comAtalho põe o atalho entre parênteses só onde ele existe', () => {
  assert.equal(comAtalho('Recolher barra lateral', 'barraLateral', MAC_APP.mac, MAC_APP.web), 'Recolher barra lateral (⌃⌘S)')
  assert.equal(comAtalho('Recolher barra lateral', 'barraLateral', MAC_WEB.mac, MAC_WEB.web), 'Recolher barra lateral', 'Web em Mac: sem rótulo de atalho')
  assert.equal(comAtalho('Recolher barra lateral', 'barraLateral', APP_FORA.mac, APP_FORA.web), 'Recolher barra lateral')
  assert.equal(comAtalho('Recolher barra lateral', 'barraLateral', WEB_FORA.mac, WEB_FORA.web), 'Recolher barra lateral')
})

test('na Web o Mac vem do navegador, não do stub de plataforma (que diz linux); no app, vem do sistema', () => {
  assert.equal(macNoAparelho(true, 'linux', 'MacIntel'), true, 'Safari ou Chrome num Mac')
  assert.equal(macNoAparelho(true, 'linux', 'iPad'), true)
  assert.equal(macNoAparelho(true, 'linux', 'Win32'), false, 'navegador no Windows')
  assert.equal(macNoAparelho(true, 'linux', 'Linux x86_64'), false)
  assert.equal(macNoAparelho(false, 'darwin', ''), true, 'app no Mac')
  assert.equal(macNoAparelho(false, 'linux', 'MacIntel'), false, 'app fora do Mac não lê o navegador')
})

test('⌘K só dispara a busca no Mac; Ctrl+K dispara fora dele, com o mesmo rótulo que o usuário vê', () => {
  const tecla = (key: string, mods: Partial<KeyboardEvent> = {}) => ({ key, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods })
  assert.equal(teclaDeBusca(tecla('k', { metaKey: true }), true), true)
  assert.equal(teclaDeBusca(tecla('k', { ctrlKey: true }), true), false, 'no Mac, Ctrl+K não é o atalho')
  assert.equal(teclaDeBusca(tecla('k', { ctrlKey: true }), false), true, 'fora do Mac, Ctrl+K é o atalho')
  assert.equal(teclaDeBusca(tecla('k', { metaKey: true }), false), false, 'fora do Mac, a tecla Meta/Windows não dispara')
  assert.equal(teclaDeBusca(tecla('K', { ctrlKey: true }), false), true, 'caixa-alta conta')
  assert.equal(teclaDeBusca(tecla('k', { ctrlKey: true, shiftKey: true }), false), false, 'Shift deixa de ser o atalho')
  assert.equal(teclaDeBusca(tecla('k', { ctrlKey: true, altKey: true }), false), false, 'Alt deixa de ser o atalho')
  assert.equal(teclaDeBusca(tecla('j', { ctrlKey: true }), false), false)
})

test('os rótulos de barra lateral nos seis idiomas não trazem o atalho fixo: ele entra pela função', () => {
  const catalogos = [ajustesPt, ajustesEn, ajustesFr, ajustesAr, ajustesZh, ajustesEs]
  for (const c of catalogos) assert.doesNotMatch(c['ajustes.aparencia.recolherBarra'], /⌃|⌘/)
  for (const c of [janelaPt, janelaEn, janelaFr, janelaAr, janelaZh, janelaEs]) {
    assert.doesNotMatch(c['navegacao.janela.recolherDica'], /⌃|⌘/)
    assert.doesNotMatch(c['navegacao.janela.mostrarDica'], /⌃|⌘/)
  }
})
