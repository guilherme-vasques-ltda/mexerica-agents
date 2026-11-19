import assert from 'node:assert/strict'
import test from 'node:test'
import { extrairProposta, extrairRascunho, LIMITES_RASCUNHO } from './rascunho.ts'

const corpo = { schema: 1, reason: 'As notas repetem o mesmo formato.\nVale guardar o procedimento.', name: 'Revisão de notas', description: 'Quando organizar notas fornecidas.', instructions: '1. Leia.\n2. Separe fatos de dúvidas.\n<script>passivo</script> https://example.test' }
const bloco = (valor: unknown = corpo, formato = 'donna-skill-recommendation') => '```' + formato + '\n' + JSON.stringify(valor) + '\n```'
const { reason: _, ...a } = corpo

test('recomendação: conteúdo íntegro e passivo, CRLF/UTF-8 e API119A preservada', () => {
  const { schema: _, ...esperado } = corpo
  assert.deepEqual(extrairProposta(('Sugestão sem pedido prévio:\n' + bloco()).replaceAll('\n', '\r\n')), { ...esperado, tipo: 'recomendacao' })
  assert.equal(extrairRascunho(bloco()), null)
  assert.deepEqual(extrairRascunho(bloco(a, 'donna-skill-draft')), { name: a.name, description: a.description, instructions: a.instructions })
})

test('recomendação: contrato exato recusa identidade/destino/aprovação, duplicadas e tipos', () => {
  for (const campo of ['sender', 'bot', 'author', 'target', 'approval', 'tools', 'script']) assert.equal(extrairProposta(bloco({ ...corpo, [campo]: 'forjado' })), null)
  for (const valor of [{ ...corpo, reason: '' }, { ...corpo, reason: ' ' }, { ...corpo, reason: 7 }, { ...corpo, schema: 2 }, { ...corpo, name: '' }, { ...corpo, description: [] }, { ...corpo, instructions: '\n' }, a, [corpo], null]) assert.equal(extrairProposta(bloco(valor)), null)
  for (const campo of Object.keys(corpo)) assert.equal(extrairProposta(bloco().replace('"schema":1', JSON.stringify(campo) + ':' + JSON.stringify(corpo[campo as keyof typeof corpo]) + ',"schema":1')), null)
  assert.equal(extrairProposta(bloco().replace('"reason":', '"rea\\u0073on":"duplicada","reason":')), null)
})

test('recomendação: limites sem truncar motivo, campos, envelope ou mensagem', () => {
  assert.ok(extrairProposta(bloco({ ...corpo, reason: '😀'.repeat(1024), name: '😀'.repeat(64), description: 'a'.repeat(1024), instructions: 'a'.repeat(32768) })))
  for (const valor of [{ ...corpo, reason: '😀'.repeat(1025) }, { ...corpo, name: 'a'.repeat(65) }, { ...corpo, description: 'a'.repeat(1025) }, { ...corpo, instructions: 'á'.repeat(16385) }]) assert.equal(extrairProposta(bloco(valor)), null)
  assert.equal(extrairProposta('```donna-skill-recommendation\n' + ' '.repeat(LIMITES_RASCUNHO.envelope) + JSON.stringify(corpo) + '\n```'), null)
  assert.equal(extrairProposta('á'.repeat(65536) + '\n' + bloco()), null)
})

test('recomendação: uma proposta máxima entre a e b1, inclusive inválida/incompleta', () => {
  for (const primeiro of [bloco(), bloco(a, 'donna-skill-draft')]) {
    for (const segundo of [bloco(), bloco(a, 'donna-skill-draft'), bloco({ ...corpo, target: 'x' }), bloco().slice(0, -3)]) {
      assert.equal(extrairProposta(primeiro + '\n' + segundo), null)
      assert.equal(extrairRascunho(primeiro + '\n' + segundo), null)
    }
  }
})

test('recomendação: citações, exemplos aninhados, decoração e EOF ficam Markdown', () => {
  for (const prefixo of ['> ', '    ', '  ']) assert.equal(extrairProposta(bloco().split('\n').map((l) => prefixo + l).join('\n')), null)
  for (const [inicio, fim] of [['````markdown', '````'], ['~~~markdown', '~~~'], ['```json', '```']]) assert.equal(extrairProposta(inicio + '\n' + bloco() + '\n' + fim), null)
  for (const texto of [bloco().slice(0, -3), bloco().slice(0, -1), bloco().replace('```donna', '````donna'), bloco().replace('recommendation', 'recommendation extra'), bloco().replace('recommendation', 'recommendation ')]) assert.equal(extrairProposta(texto), null)
})
