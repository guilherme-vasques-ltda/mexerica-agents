import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { environmentProfile } from './environment.ts'
import { itensMenuApp } from './fork-menu-app.ts'

// D443 (antes D348): o menu do app no macOS mostra "Mexerica", com artigo no Sobre (D445a); os papéis e o nome técnico ficam.
test('D443: o menu do app diz Sobre a/Ocultar/Encerrar Mexerica e mantém os papéis', () => {
  let ajustes = 0
  const itens = itensMenuApp('Donnabot', () => ajustes++, 'pt-BR')
  const papel = (role: string) => itens.find((item) => 'role' in item && item.role === role)
  assert.deepEqual(papel('about'), { role: 'about', label: 'Sobre a Mexerica' })
  assert.deepEqual(papel('hide'), { role: 'hide', label: 'Ocultar Mexerica' })
  assert.deepEqual(papel('quit'), { role: 'quit', label: 'Encerrar Mexerica' })
  for (const role of ['services', 'hideOthers', 'unhide']) assert.ok(papel(role), role)
  const rotulos = itens.flatMap((item) => ('label' in item && item.label ? [item.label] : []))
  assert.ok(rotulos.every((rotulo) => !/Donnabot|DONNA Bot|Donna Bot/.test(rotulo)), rotulos.join(' | '))
  const abrir = itens.find((item) => 'label' in item && item.label === 'Ajustes…')
  assert.ok(abrir && 'click' in abrir && abrir.accelerator === 'CmdOrCtrl+,')
  abrir.click()
  assert.equal(ajustes, 1)
  const sair = itensMenuApp('Donnabot Dev', () => {}, 'pt-BR').find((item) => 'role' in item && item.role === 'quit')
  assert.deepEqual(sair, { role: 'quit', label: 'Encerrar Mexerica Dev' })
})

test('D348/D443: o nome técnico continua Donnabot (app.name e o perfil de identidade); o pacote se chama Mexerica (PR d)', () => {
  assert.equal(environmentProfile('main').name, 'Donnabot')
  assert.equal(environmentProfile('main').appId, 'app.donnabot.mac')
  assert.equal(environmentProfile('main').dataFolder, '.mexerica')
  assert.equal(environmentProfile('dev').name, 'Donnabot Dev')
  const builder = readFileSync(new URL('../../electron-builder.yml', import.meta.url), 'utf8')
  assert.match(builder, /^productName: Mexerica$/m)
  assert.match(builder, /^appId: app\.donnabot\.mac$/m)
  // O menu só lê o nome: nada nele chama app.setName nem muda o userData.
  assert.doesNotMatch(readFileSync(new URL('./fork-menu-app.ts', import.meta.url), 'utf8'), /setName|setPath|userData/)
})

// D454/D495 (R1 do #563): o menu segue o idioma efetivo do app; sem escolha, o padrão inglês.
test('D454/D495: sem idioma escolhido o menu do Mac sai em inglês; com pt-BR, em português', () => {
  const rotulos = (idioma?: 'pt-BR' | 'en' | 'fr') => itensMenuApp('Donnabot', () => {}, idioma).flatMap((i) => ('label' in i && i.label ? [i.label] : []))
  assert.deepEqual(rotulos(), ['About Mexerica', 'Settings…', 'Hide Mexerica', 'Quit Mexerica'])
  assert.deepEqual(rotulos('pt-BR'), ['Sobre a Mexerica', 'Ajustes…', 'Ocultar Mexerica', 'Encerrar Mexerica'])
  assert.deepEqual(rotulos('fr'), ['À propos de Mexerica', 'Réglages…', 'Masquer Mexerica', 'Quitter Mexerica'])
})
