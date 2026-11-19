// DON-917 (fatia 9d, D70): hora e contagens do Mission Board no idioma, sem trocar a ordem da frase em árabe.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { traduzirPlural } from '../../../lib/fork-i18n/index.ts'
import { railLabel } from './badge.ts'
import { haQuanto, hora } from './formato.ts'

const LRI = '⁦', PDI = '⁩'

test('a hora sai no formato do idioma e isolada LTR; "há quanto tempo" vem do Intl do idioma', () => {
  const ms = Date.UTC(2026, 10, 8, 14, 5)
  for (const idioma of ['ar-AE', 'en', 'pt-BR'] as const) {
    const h = hora(ms, idioma)
    assert.ok(h.startsWith(LRI) && h.endsWith(PDI), idioma)
  }
  assert.equal(haQuanto({ unit: 'now' }, 'en'), 'a moment ago')
  assert.match(haQuanto({ unit: 'minute', n: 18 }, 'en'), /18 min/)
  assert.notEqual(haQuanto({ unit: 'day', n: 2 }, 'ar-AE'), haQuanto({ unit: 'day', n: 2 }, 'en'))
})

test('fila vazia diz "nenhuma" (D507) nos seis idiomas e o rótulo da faixa segue o idioma', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    assert.doesNotMatch(traduzirPlural(idioma, 'torre.fila.esperando', 0), /\b0\b/, idioma)
  }
  assert.equal(traduzirPlural('en', 'torre.fila.esperando', 0), 'None waiting')
  assert.equal(traduzirPlural('ar-AE', 'torre.permissoes', 2), 'إذنان ينتظرانك')
})

// Decisão do dono (opção A): o nome "Mission Board" é igual nos seis idiomas, numa chave própria (`torre.nome`), e o
// texto em volta é traduzido; na frase o nome vai isolado (FSI…PDI). O RailButton usa este rótulo em title e aria-label.
test('tooltip da faixa: o nome fica igual, o texto em volta segue o idioma, com e sem pendências, nos seis idiomas', async () => {
  const { readFileSync } = await import('node:fs')
  const FSI = '\u2068'
  const esperado: Record<string, RegExp> = {
    'pt-BR': /, 3 esperando você$/, en: /, 3 waiting for you$/, es: /, 3 esperándote$/, fr: /, 3 vous attendent$/,
    // Em árabe o interpolador isola cada parâmetro (FSI…PDI), inclusive a contagem.
    'zh-CN': /，3 个等待你处理$/, 'ar-AE': /، \u2068?3\u2069? في انتظارك$/,
  }
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    for (const n of [null, 0]) assert.equal(railLabel(n, idioma), 'Mission Board', `${idioma} sem pendência`)
    const com = railLabel(3, idioma)
    assert.ok(com.includes(`${FSI}Mission Board${PDI}`) && com.indexOf('Mission Board') <= 2, `${idioma}: nome isolado no começo`)
    assert.match(com, esperado[idioma]!, idioma)
  }
  // O botão usa o mesmo rótulo, no idioma da tela, no title e no aria-label.
  const botao = readFileSync(new URL('./RailButton.tsx', import.meta.url), 'utf8')
  assert.match(botao, /const label = railLabel\(n, useIdioma\(\)\)/)
  assert.match(botao, /title=\{label\} aria-label=\{label\}/)
})
