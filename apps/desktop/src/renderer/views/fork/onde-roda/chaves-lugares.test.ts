import { test } from 'node:test'
import assert from 'node:assert/strict'
import { abrirEstado, escreverEstado, type EstadoGuardado } from '../../settings/estado-pagina.ts'
import { chaveCadastro, chaveRascunho, chaveSelecionado } from './chaves.ts'

// Lugares monta um bloco por Hub ao mesmo tempo. Digitar no Hub A, ir ao Hub B e voltar ao A é a sequência do
// hook (estado-pagina.ts) com as chaves de Lugares: o B vê o próprio estado vazio, e o A volta com o texto dele.

/** O que o hook faz numa digitação: muda o valor e o efeito grava no mapa do pai. */
function digitar(guardado: EstadoGuardado, chave: string, texto: string) {
  const estado = escreverEstado(abrirEstado(guardado, chave, ''), chave, guardado, '', texto)
  guardado.set(estado.chave, estado.valor)
}

test('o formulário do Hub B vem vazio depois de digitar no Hub A; o do A volta com o texto', () => {
  const guardado: EstadoGuardado = new Map()
  digitar(guardado, chaveCadastro('hub-a', 'nome'), 'Recurso do Hub A')
  assert.equal(abrirEstado(guardado, chaveCadastro('hub-b', 'nome'), '').valor, '')
  assert.equal(abrirEstado(guardado, chaveCadastro('hub-a', 'nome'), '').valor, 'Recurso do Hub A')
})

test('o recurso digitado também fica por Hub', () => {
  const guardado: EstadoGuardado = new Map()
  digitar(guardado, chaveCadastro('hub-a', 'recurso'), 'bx_aaaa')
  assert.equal(abrirEstado(guardado, chaveCadastro('hub-b', 'recurso'), '').valor, '')
  assert.equal(abrirEstado(guardado, chaveCadastro('hub-a', 'recurso'), '').valor, 'bx_aaaa')
})

test('a seleção e o rascunho de um lugar não passam para o mesmo lugar em outro Hub', () => {
  const guardado: EstadoGuardado = new Map()
  digitar(guardado, chaveSelecionado('hub-a'), 'lugar-1')
  digitar(guardado, chaveRascunho('hub-a', 'lugar-1'), 'nome editado no A')
  assert.equal(abrirEstado(guardado, chaveSelecionado('hub-b'), null).valor, null)
  assert.equal(abrirEstado(guardado, chaveRascunho('hub-b', 'lugar-1'), 'padrão').valor, 'padrão')
  assert.equal(abrirEstado(guardado, chaveRascunho('hub-a', 'lugar-1'), 'padrão').valor, 'nome editado no A')
})

test('chaves de Hubs diferentes nunca são iguais', () => {
  assert.notEqual(chaveCadastro('hub-a', 'nome'), chaveCadastro('hub-b', 'nome'))
  assert.notEqual(chaveSelecionado('hub-a'), chaveSelecionado('hub-b'))
  assert.notEqual(chaveRascunho('hub-a', 'p'), chaveRascunho('hub-b', 'p'))
})

test('se a chave muda sem remontar, a mudança parte do estado da chave nova, não da anterior', () => {
  const guardado: EstadoGuardado = new Map()
  // Estado ainda da chave do A (o que estava na tela antes da troca de Hub).
  const antes = { chave: chaveCadastro('hub-a', 'nome'), valor: 'A' }
  const depois = escreverEstado(antes, chaveCadastro('hub-b', 'nome'), guardado, '', (anterior: string) => `${anterior}!`)
  assert.equal(depois.chave, chaveCadastro('hub-b', 'nome'))
  assert.equal(depois.valor, '!', 'o B começa do vazio, não do texto do A')
})
