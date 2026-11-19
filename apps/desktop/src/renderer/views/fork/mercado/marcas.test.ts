import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { arquivoDaMarca, MARCAS } from '../../marketplace/marcas.ts'

const aqui = dirname(fileURLToPath(import.meta.url))

test('recorte sem logos de terceiros: nenhuma marca resolve e todo serviço usa o globo', () => {
  assert.deepEqual(Object.keys(MARCAS), [])
  for (const chave of ['linear', 'stripe', 'slack', '', null, undefined, '__proto__', 'constructor']) assert.equal(arquivoDaMarca(chave), null)
})

test('a pasta de logos de terceiros não existe no recorte', () => {
  assert.equal(existsSync(join(aqui, '../../../assets/marcas')), false)
})
