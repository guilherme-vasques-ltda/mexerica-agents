import assert from 'node:assert/strict'
import test from 'node:test'
import { marcarPassoIdioma, ORDEM_RODA, passoIdiomaPendente, teclaRoda } from './passo.ts'

const tecla = (key: string, mod: Partial<{ altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {}) =>
  ({ key, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...mod })

test('a roda: setas sem dar a volta, Home/End nas pontas, Enter continua', () => {
  // D454/D495: English primeiro, Português (Brasil) em segundo, depois os outros na ordem de antes.
  assert.deepEqual(ORDEM_RODA, ['en', 'pt-BR', 'es', 'fr', 'zh-CN', 'ar-AE'])
  assert.deepEqual(teclaRoda(tecla('ArrowDown'), 'en'), { escolher: 'pt-BR' })
  assert.deepEqual(teclaRoda(tecla('ArrowDown'), 'ar-AE'), { escolher: 'ar-AE' })
  assert.deepEqual(teclaRoda(tecla('ArrowUp'), 'es'), { escolher: 'pt-BR' })
  assert.deepEqual(teclaRoda(tecla('ArrowUp'), 'en'), { escolher: 'en' })
  assert.deepEqual(teclaRoda(tecla('Home'), 'pt-BR'), { escolher: 'en' })
  assert.deepEqual(teclaRoda(tecla('End'), 'en'), { escolher: 'ar-AE' })
  // A roda é vertical: ↑/↓ não trocam de sentido em RTL.
  assert.deepEqual(teclaRoda(tecla('Enter'), 'en'), { continuar: true })
  assert.equal(teclaRoda(tecla('ArrowDown', { metaKey: true }), 'en'), null)
  assert.equal(teclaRoda(tecla('Tab'), 'en'), null)
})

test('o passo aparece até Continuar e some depois; sem armazenamento, não segura a conta', () => {
  const salvo = new Map<string, string>()
  const storage = { getItem: (k: string) => salvo.get(k) ?? null, setItem: (k: string, v: string) => void salvo.set(k, v) }
  assert.equal(passoIdiomaPendente(storage), true)
  marcarPassoIdioma(storage)
  assert.equal(passoIdiomaPendente(storage), false)
  assert.equal(passoIdiomaPendente(null), false)
  const quebrado = { getItem: () => { throw new Error('bloqueado') }, setItem: () => { throw new Error('bloqueado') } }
  assert.equal(passoIdiomaPendente(quebrado), false)
  assert.doesNotThrow(() => marcarPassoIdioma(quebrado))
})
