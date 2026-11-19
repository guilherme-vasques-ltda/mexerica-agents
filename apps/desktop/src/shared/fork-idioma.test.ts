import assert from 'node:assert/strict'
import test from 'node:test'
import { direcaoDe, isAppLanguage, sugerirIdioma } from './fork-idioma.ts'

test('sugere o primeiro idioma do sistema que o app tem', () => {
  assert.equal(sugerirIdioma(['pt-BR', 'en-US']), 'pt-BR')
  assert.equal(sugerirIdioma(['pt-PT']), 'pt-BR')
  assert.equal(sugerirIdioma(['de-DE', 'en-GB', 'pt-BR']), 'en')
  assert.equal(sugerirIdioma(['en_US']), 'en')
  assert.equal(sugerirIdioma(['ja-JP', 'de']), null)
  assert.equal(sugerirIdioma(['es-MX']), 'es')
  assert.equal(sugerirIdioma(['fr-CA']), 'fr')
  assert.equal(sugerirIdioma(['ar-SA']), 'ar-AE')
  assert.equal(sugerirIdioma(['zh-Hans-CN']), 'zh-CN')
  assert.equal(sugerirIdioma(['zh']), 'zh-CN')
  assert.equal(sugerirIdioma(['zh-Hans-HK']), 'zh-CN')
  // Chinês tradicional não recebe o simplificado: segue para o próximo idioma da lista.
  assert.equal(sugerirIdioma(['zh-Hant-TW', 'en']), 'en')
  assert.equal(sugerirIdioma(['zh-TW']), null)
  assert.equal(sugerirIdioma(['zh-HK']), null)
  assert.equal(sugerirIdioma([]), null)
})

test('ignora o que não é etiqueta de idioma', () => {
  assert.equal(sugerirIdioma([42, '', 'pt BR', '../en', 'x'.repeat(40), 'de', 'en']), 'en')
  assert.equal(isAppLanguage('en'), true)
  assert.equal(isAppLanguage('en-US'), false)
})

test('só o árabe é da direita para a esquerda', () => {
  assert.equal(direcaoDe('ar-AE'), 'rtl')
  for (const l of ['pt-BR', 'en', 'es', 'fr', 'zh-CN'] as const) assert.equal(direcaoDe(l), 'ltr')
})
