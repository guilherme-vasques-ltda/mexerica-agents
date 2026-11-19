import assert from 'node:assert/strict'
import test from 'node:test'
import { extrairRascunho, LIMITES_RASCUNHO } from './rascunho.ts'

const corpo = { schema: 1, name: 'Revisão de notas', description: 'Quando organizar notas fornecidas.', instructions: '1. Leia as notas.\n2. Separe fatos e dúvidas.' }
const bloco = (valor: unknown = corpo) => '```donna-skill-draft\n' + JSON.stringify(valor) + '\n```'

test('rascunho: uma proposta fechada entre textos, UTF-8 e CRLF; texto de instrução permanece passivo', () => {
  const c = { ...corpo, instructions: 'Texto: {"tools":["shell"]}\nhttps://example.test\n<script>não executar</script>' }
  assert.deepEqual(extrairRascunho(('Vou propor:\n\n' + bloco(c) + '\n\nConfira antes de salvar.').replaceAll('\n', '\r\n')), { name: c.name, description: c.description, instructions: c.instructions })
})

test('rascunho: schema exato, sem extras, duplicadas, arrays, tipos errados ou strings vazias', () => {
  for (const invalido of [{ ...corpo, tools: [] }, { ...corpo, url: 'https://example.test' }, { ...corpo, script: 'x' }, { ...corpo, schema: 2 }, { ...corpo, schema: '1' }, { ...corpo, name: ' ' }, { ...corpo, description: '' }, { ...corpo, instructions: '\n' }, { ...corpo, instructions: [] }, [corpo], null]) assert.equal(extrairRascunho(bloco(invalido)), null)
  assert.equal(extrairRascunho(bloco().replace('"schema":1', '"schema":1,"schema":1')), null)
})

test('rascunho: limites de caracteres, bytes UTF-8, envelope e mensagem recusam tudo, sem corte', () => {
  assert.ok(extrairRascunho(bloco({ ...corpo, name: '😀'.repeat(64), description: 'a'.repeat(1024), instructions: 'a'.repeat(32768) })))
  for (const c of [{ ...corpo, name: 'a'.repeat(65) }, { ...corpo, description: 'a'.repeat(1025) }, { ...corpo, instructions: 'á'.repeat(16385) }]) assert.equal(extrairRascunho(bloco(c)), null)
  assert.equal(extrairRascunho('```donna-skill-draft\n' + ' '.repeat(LIMITES_RASCUNHO.envelope) + JSON.stringify(corpo) + '\n```'), null)
  assert.equal(extrairRascunho('á'.repeat(65536) + '\n' + bloco()), null)
  assert.equal(extrairRascunho('a'.repeat(LIMITES_RASCUNHO.mensagem) + bloco()), null)
})

test('rascunho: EOF, fechamento curto, linguagem diferente ou decorada não viram proposta', () => {
  for (const texto of [bloco().slice(0, -3), bloco().slice(0, -1), bloco().replace('```donna', '````donna'), bloco().replace('donna-skill-draft', 'json'), bloco().replace('donna-skill-draft', 'Donna-skill-draft'), bloco().replace('donna-skill-draft', 'donna-skill-draft extra'), bloco().replace('donna-skill-draft', 'donna-skill-draft ')]) assert.equal(extrairRascunho(texto), null)
})

test('rascunho: citações, indentação e exemplos dentro de outros fences ficam só texto', () => {
  assert.equal(extrairRascunho(bloco().split('\n').map((l) => '> ' + l).join('\n')), null)
  assert.equal(extrairRascunho(bloco().split('\n').map((l) => '    ' + l).join('\n')), null)
  for (const [inicio, fim] of [['````markdown', '````'], ['`````', '`````'], ['~~~markdown', '~~~'], ['```json', '```']]) {
    // Com backticks triplos externos, o exemplo interno fecha esse bloco; não abre uma proposta.
    assert.equal(extrairRascunho(inicio + '\n' + bloco() + '\n' + fim), null)
  }
  assert.ok(extrairRascunho('````markdown\n' + bloco() + '\n````\n' + bloco()))
})

test('rascunho: duas propostas, inclusive segunda incompleta ou inválida, são ambíguas', () => {
  for (const segunda of [bloco(), bloco({ ...corpo, tools: [] }), bloco().slice(0, -3)]) assert.equal(extrairRascunho(bloco() + '\n' + segunda), null)
})
