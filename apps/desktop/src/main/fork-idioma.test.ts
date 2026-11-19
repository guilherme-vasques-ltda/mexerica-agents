import assert from 'node:assert/strict'
import test from 'node:test'
import { loadMainModule } from '../shared/test-support/main-module.ts'

// D454/D495 (R1 do #563), D70: o processo principal guarda o idioma efetivo do app que o renderer manda. Sem aviso
// (ou sem escolha salva, que o renderer manda como 'en'), o padrão é o inglês; valor fora dos 6 é ignorado.
function carregar() {
  return loadMainModule<typeof import('./fork-idioma.ts')>('fork-idioma', { electron: { app: { getPreferredSystemLanguages: () => [] } } })
}

test('D454/D495: o main começa em inglês e troca só para um dos 6 idiomas, avisando quem redesenha', () => {
  const m = carregar()
  assert.equal(m.idiomaDoApp(), 'en')
  let avisos = 0
  const sair = m.aoMudarIdiomaDoApp(() => avisos++)
  assert.equal(m.definirIdiomaDoApp('en'), false, 'o mesmo idioma não refaz o menu')
  assert.equal(m.definirIdiomaDoApp('pt-BR'), true)
  assert.equal(m.idiomaDoApp(), 'pt-BR')
  assert.equal(m.definirIdiomaDoApp('de'), false)
  assert.equal(m.definirIdiomaDoApp(null), false)
  assert.equal(m.idiomaDoApp(), 'pt-BR')
  assert.equal(avisos, 1)
  sair()
  m.definirIdiomaDoApp('fr')
  assert.equal(avisos, 1)
})
