import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import { ehHubDesteComputador } from './aviso-instalacao-local.ts'

test('o aviso vale só para o Hub deste computador', () => {
  const local = { computer: { id: 'computador-1' } }
  assert.equal(ehHubDesteComputador(local, { computer: { id: 'computador-1' } }), true)
  assert.equal(ehHubDesteComputador(local, { computer: { id: 'vps-1' } }), false)
  assert.equal(ehHubDesteComputador(null, { computer: { id: 'computador-1' } }), false)
})

test('o texto do aviso existe nos seis idiomas, sem marcador sobrando', () => {
  for (const [idioma, catalogo] of Object.entries(CATALOGOS)) {
    const texto = catalogo['avisoLocal.texto']
    assert.ok(texto && texto.trim().length > 0, idioma)
    assert.ok(!/[{}]/.test(texto), idioma)
  }
})
