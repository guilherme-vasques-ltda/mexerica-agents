// D70 (auditoria D249, E3): só o roteamento da tela de Extensões.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ehPaginaLoja, PAGINAS_EXTENSOES, TelaExtensoes } from './tela.ts'

const tick = () => new Promise<void>((r) => queueMicrotask(r))

test('cada atalho abre a sua página; sem computador não abre', () => {
  const tela = new TelaExtensoes()
  tela.show('connectors', null)
  assert.equal(tela.open, false, 'sem Hub conhecido não há o que mostrar')
  tela.show('connectors', 'hub-1')
  assert.equal(tela.page, 'connectors')
  tela.show('connectors', 'hub-1')
  tela.show('skills', 'hub-1')
  assert.equal(tela.page, 'skills')
})

test('Open mantém Conectores e Skills acessíveis e recusa Plugins e Credenciais', (t) => {
  const mockWindow = { codync: { appScheme: 'mexerica-open' } }
  Object.defineProperty(globalThis, 'window', { configurable: true, value: mockWindow })
  t.after(() => { Reflect.deleteProperty(globalThis, 'window') })
  const tela = new TelaExtensoes()
  tela.show('plugins', 'hub-1', 'whatsapp')
  assert.equal(tela.open, false)
  tela.show('skills', 'hub-1')
  assert.equal(tela.page, 'skills', 'a tela de Skills abre na Open')
  for (const page of ['plugins', 'credentials'] as const) {
    tela.setPage(page)
    assert.equal(tela.page, 'skills')
  }
  tela.setPage('connectors')
  assert.equal(tela.page, 'connectors')
})

test('abrir fecha o lugar atual; fechar pelo Voltar devolve o foco e ir a outro lugar não', () => {
  const tela = new TelaExtensoes()
  let abriu = 0
  let devolveu = 0
  tela.aoAbrir = () => { abriu++ }
  tela.aoFechar = () => { devolveu++ }
  tela.show('connectors', 'hub-1')
  assert.equal(abriu, 1)
  tela.close(false)
  assert.equal(tela.open, false)
  assert.equal(devolveu, 0)
  tela.show('connectors', 'hub-1')
  tela.close()
  assert.equal(devolveu, 1)
  tela.close()
  assert.equal(devolveu, 1, 'fechada, fechar de novo não faz nada')
})

test('página e computador só mudam com a tela aberta', async () => {
  const tela = new TelaExtensoes()
  let avisos = 0
  tela.subscribe(() => { avisos++ })
  tela.setPage('skills')
  tela.setComputer('hub-2')
  assert.equal(tela.open, false)
  await tick()
  assert.equal(avisos, 0)
  tela.show('connectors', 'hub-1')
  await tick()
  tela.setPage('connectors')
  await tick()
  assert.equal(avisos, 1, 'a mesma página não avisa')
  tela.setPage('skills')
  tela.setComputer('hub-2')
  assert.deepEqual([tela.page, tela.computer], ['skills', 'hub-2'])
})

test('Entrega 2: Plugins e Credenciais são páginas; a loja só desenha Conectores e Skills (Aplicativos virou Conectores)', () => {
  assert.deepEqual(PAGINAS_EXTENSOES.map((p) => p.id), ['connectors', 'skills', 'plugins', 'credentials'])
  assert.deepEqual(PAGINAS_EXTENSOES.filter((p) => ehPaginaLoja(p.id)).map((p) => p.id), ['connectors', 'skills'])
})

test('Entrega 2: o plugin só abre na página Plugins e fecha ao trocar de página, ao reabrir e ao fechar a tela', async () => {
  const tela = new TelaExtensoes()
  tela.show('skills', 'hub-1')
  tela.setPlugin('whatsapp')
  assert.equal(tela.plugin, null, 'fora de Plugins, nada abre')
  tela.setPage('plugins')
  assert.equal(tela.plugin, null, 'Plugins abre em lista fechada')
  tela.setPlugin('whatsapp')
  assert.equal(tela.plugin, 'whatsapp')
  let avisos = 0
  tela.subscribe(() => { avisos++ })
  tela.setPage('plugins')
  await tick()
  assert.deepEqual([tela.plugin, avisos], [null, 1], 'clicar em Plugins de novo volta à lista')
  tela.setPlugin('whatsapp')
  tela.setPage('credentials')
  assert.equal(tela.plugin, null)
  tela.show('plugins', 'hub-1', 'whatsapp')
  assert.equal(tela.plugin, 'whatsapp', 'o atalho do bot abre direto no plugin')
  tela.show('connectors', 'hub-1', 'whatsapp')
  assert.equal(tela.plugin, null, 'plugin pedido em outra página é ignorado')
  tela.show('plugins', 'hub-1', 'whatsapp')
  tela.close()
  assert.equal(tela.plugin, null)
})
