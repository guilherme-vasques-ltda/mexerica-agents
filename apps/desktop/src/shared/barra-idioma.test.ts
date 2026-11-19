import assert from 'node:assert/strict'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

// D454 (fatia 10b), D70: o menu da barra do Mac sai do catálogo no idioma do app, num mecanismo só (a tabela pt-BR do
// #563 saiu). Nome e detalhe de bot passam como vieram, mesmo iguais a um rótulo; o idioma muda o menu inteiro.
type Item = { label?: string; submenu?: Item[] }

function montar(idioma: { atual: string }, resumo: Record<string, unknown>) {
  let menu: Item[] = []
  const imagem = { setTemplateImage() {}, resize() { return imagem }, addRepresentation() {}, isEmpty: () => true }
  const m = loadMainModule<{ Tray: new (deps: unknown) => { update(s: unknown): void } }>('tray', {
    electron: {
      app: { getName: () => 'Donnabot', isPackaged: true, quit() {} },
      Menu: { buildFromTemplate: (t: Item[]) => { menu = t; return t } },
      nativeImage: { createFromPath: () => imagem, createEmpty: () => imagem },
      shell: { openPath() {} },
      Tray: class { setToolTip() {} on() {} setImage() {} setContextMenu() {} },
    },
    'node:path': { join: (...p: string[]) => p.join('/') },
    './dock': { showInDock: () => true, setShowInDock() {} },
    './login-item': { launchAtLogin: () => false, setLaunchAtLogin: (v: boolean) => v },
    './island': { islandSupported: true },
    './environment': { identity: { environment: 'release' } },
    './fork-idioma': { idiomaDoApp: () => idioma.atual },
  }, { process: { platform: 'darwin', resourcesPath: '/r' } })
  const tray = new m.Tray({
    openChat() {}, openPairing() {}, send() {},
    host: { state: { kind: 'running' }, logPath: '/log' },
    updates: { state: { supported: true, autoCheck: true, autoDownload: false, installing: false, staged: false, downloadPercent: null, checking: false, availableVersion: '2.0.0', lastCheck: null, error: null }, check() {}, emit() {} },
    island: { enabled: false, setEnabled() {} },
  })
  tray.update({ status: 'X', canPair: true, approval: null, bots: [], usage: [], screen: null, version: '1.2.3', needsAttention: false, textSize: 12, usageIconStyle: 'character', ...resumo })
  return () => menu
}

const rotulos = (itens: Item[]): string[] => itens.flatMap((i) => [...(i.label ? [i.label] : []), ...rotulos(i.submenu ?? [])])

test('menu da barra: os rótulos saem do catálogo no idioma do app e trocam com ele', () => {
  const idioma = { atual: 'en' }
  const menu = montar(idioma, {})
  const en = rotulos(menu())
  for (const r of ['Open Mexerica', 'Quit Mexerica', 'Settings', 'Version 1.2.3', 'Update to 2.0.0', '12 pt (default)', 'Show in notch']) assert.ok(en.includes(r), r)
  idioma.atual = 'pt-BR'
  const pt = rotulos(montar(idioma, {})())
  for (const r of ['Abrir a Mexerica', 'Sair da Mexerica', 'Ajustes', 'Versão 1.2.3', 'Atualizar para 2.0.0', '12 pt (padrão)', 'Mostrar no entalhe']) assert.ok(pt.includes(r), r)
  idioma.atual = 'ar-AE'
  const ar = rotulos(montar(idioma, {})())
  assert.ok(ar.includes('الإعدادات'))
  assert.equal(ar.filter((r) => /[A-Za-z]{4,}/.test(r) && !/Mexerica|Dock|iPhone|pt/.test(r)).length, 0, ar.join(' | '))
})

test('menu da barra: nome e detalhe de bot ficam como vieram, mesmo iguais a um rótulo', () => {
  const menu = montar({ atual: 'pt-BR' }, { bots: [{ ref: 'c/b', name: 'Settings', detail: 'Open conversation', working: false, avatar: null }] })
  const bot = menu().find((i) => i.label === 'Settings')
  assert.ok(bot, 'o nome do bot não vira "Ajustes"')
  assert.equal(JSON.stringify(bot!.submenu!.map((i) => i.label)), JSON.stringify(['Open conversation', 'Abrir conversa']))
})
