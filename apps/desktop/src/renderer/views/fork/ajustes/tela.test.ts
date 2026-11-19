// D70 (ajustes-tela-01): só o roteamento da tela de Ajustes.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { TelaAjustes, nomeDoSpaceAtivo, voltarDosAjustes } from './tela.ts'

const tick = () => new Promise<void>((r) => queueMicrotask(r))

test('abre em Geral sem página, ou na página pedida', () => {
  const tela = new TelaAjustes()
  assert.equal(tela.open, false)
  tela.show()
  assert.equal(tela.page, 'general')
  tela.show('storage')
  assert.equal(tela.page, 'storage')
  assert.equal(tela.aba, null, 'página sem abas não tem aba')
  // MX-145 E2: Uso abre em Por conta e lembra a aba escolhida.
  tela.show('usage')
  assert.equal(tela.aba, 'conta')
  tela.setAba('modelo')
  assert.equal(tela.aba, 'modelo')
})

test('abre na aba pedida; aba desconhecida cai na primeira; a última aba volta ao reabrir', () => {
  const tela = new TelaAjustes()
  tela.show('ai-accounts', 'motores')
  assert.equal(tela.aba, 'motores')
  tela.close()
  tela.show('ai-accounts')
  assert.equal(tela.aba, 'motores', 'lembra a última aba da sessão')
  tela.setAba('nao-existe')
  assert.equal(tela.aba, 'motores')
  tela.close()
  tela.show('ai-accounts', 'nao-existe')
  assert.equal(tela.aba, 'contas', 'aba pedida e desconhecida cai na primeira, mesmo com outra lembrada')
  const nova = new TelaAjustes()
  nova.show('ai-accounts', 'nao-existe')
  assert.equal(nova.aba, 'contas')
  nova.setAba('motores')
  nova.setPage('general')
  assert.equal(nova.aba, null)
  nova.setPage('ai-accounts')
  assert.equal(nova.aba, 'motores')
})

test('Voz tem três abas: abre na primeira; o botão da chamada abre em Ao vivo', () => {
  const tela = new TelaAjustes()
  tela.show('voice')
  assert.equal(tela.aba, 'ouvir')
  tela.show('voice', 'vivo')
  assert.equal(tela.aba, 'vivo')
  tela.setAba('chaves')
  assert.equal(tela.aba, 'chaves')
})

test('fechar fecha uma vez; fechada, trocar de página não abre', async () => {
  const tela = new TelaAjustes()
  let avisos = 0
  tela.subscribe(() => avisos++)
  tela.setPage('voice')
  assert.equal(tela.open, false)
  tela.show('usage')
  await tick()
  tela.close()
  await tick()
  assert.equal(avisos, 2)
  tela.close()
  await tick()
  assert.equal(avisos, 2, 'fechar o que já está fechado não avisa')
})

test('Voltar e Esc devolvem o foco; ir a outro lugar não', () => {
  const tela = new TelaAjustes()
  let foco = 0
  tela.aoFechar = () => foco++
  tela.show()
  voltarDosAjustes(tela)
  assert.equal(tela.open, false)
  assert.equal(foco, 1)
  tela.show()
  tela.close(false)
  assert.equal(foco, 1)
})

test('abrir Ajustes fecha o lugar aberto antes (Rotinas)', () => {
  const tela = new TelaAjustes()
  let rotinas = true
  tela.aoAbrir = () => { rotinas = false }
  tela.show('voice')
  assert.equal(rotinas, false)
  assert.equal(tela.page, 'voice')
})

test('cabeçalho da lateral: o nome do Space ativo; Pessoal só sem Space ativo', () => {
  assert.equal(nomeDoSpaceAtivo(null, [{ id: 's1', name: 'Empresa' }]), null)
  assert.equal(nomeDoSpaceAtivo({ spaceId: 's1', record: { name: 'Empresa' } }, undefined), 'Empresa')
  assert.equal(nomeDoSpaceAtivo({ spaceId: 's1', record: null }, [{ id: 's0', name: 'Outro' }, { id: 's1', name: 'Loja' }]), 'Loja', 'sem o registro ainda, pela lista do computador')
  assert.equal(nomeDoSpaceAtivo({ spaceId: 's9', record: null }, [{ id: 's1', name: 'Loja' }]), null, 'nunca o nome de outro Space')
})
