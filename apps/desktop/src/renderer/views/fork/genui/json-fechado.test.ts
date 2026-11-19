import assert from 'node:assert/strict'
import test from 'node:test'
import { parseClosedJson } from './json-fechado.ts'

test('JSON fechado recusa duplicatas em objetos aninhados e arrays, decodificando as chaves', () => {
  for (const raw of [
    '{"title":1,"title":2}',
    '{"data":{"question":1,"quest\\u0069on":2}}',
    '{"options":[{"label":1,"\\u006cabel":2}]}',
    '{"a":[[{"\\uD83D\\uDE00":1,"😀":2}]]}',
    '{"a":{"x":{},"x":[]}}',
    '{"__proto__":1,"\\u005f_proto__":2}',
  ]) assert.deepEqual(parseClosedJson(raw), { ok: false, error: 'genui_invalid_payload' })
})

test('JSON padrão preserva dados e strings inertes, com escopos separados; sintaxe inválida falha sem eco', () => {
  const raw = '{"a":[{"label":"\\\" : { } [ ] \\\\"},{"label":"<script>sentinela</script>"}],"label":-1.25e+3}'
  assert.deepEqual(parseClosedJson(raw), { ok: true, value: JSON.parse(raw) })
  for (const raw of ['{"sentinela":', '[1,]', '{"a":1} {"a":2}', '"\\x20"']) {
    assert.deepEqual(parseClosedJson(raw), { ok: false, error: 'genui_invalid_payload' })
  }
})

test('varredura não depende de recursão para percorrer JSON válido profundo', () => {
  const raw = '['.repeat(12_000) + '{"a":1,"\\u0061":2}' + ']'.repeat(12_000)
  assert.deepEqual(parseClosedJson(raw), { ok: false, error: 'genui_invalid_payload' })
  assert.equal(parseClosedJson(raw.replace(',"\\u0061":2', '')).ok, true)
})

test('uint é opção explícita por campo, inclusive escapado e em snapshot; floats restantes continuam válidos', () => {
  const options = { unsignedIntegerFields: ['version'] }
  for (const key of ['version', 'vers\\u0069on']) {
    for (const token of ['1.0', '1e0', '-0', '-1', '"1"', 'null', 'true', '{}', '[]']) {
      const raw = `{"payload":{"${key}" : \n${token}},"data":{"values":[-0,1.5,-2.5,1e0,2e-3]}}`
      assert.deepEqual(parseClosedJson(raw), { ok: true, value: JSON.parse(raw) })
      assert.deepEqual(parseClosedJson(raw, options), { ok: false, error: 'genui_invalid_payload' })
    }
    // Faixa e schema são do consumidor, não do helper lexical genérico.
    const raw = `{"${key}":256,"payload":{"${key}":1},"data":{"values":[-0,1.5,-2.5,1e0,2e-3]}}`
    assert.deepEqual(parseClosedJson(raw, options), { ok: true, value: JSON.parse(raw) })
  }
})
