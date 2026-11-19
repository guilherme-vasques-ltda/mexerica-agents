import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { APP_LANGUAGES } from '../../../../shared/fork-idioma.ts'
import { traduzirPlural } from '../../../lib/fork-i18n/index.ts'

// `voice-settings` importa o estado do idioma (sem extensão): sai num bundle, como nas outras telas.
const bundle = await build({
  stdin: { contents: "export { languageOptions } from './views/settings/voice-settings'", resolveDir: new URL('../../..', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  define: { 'process.env.NODE_ENV': '"production"' },
})
const window = { codync: { platform: 'darwin' }, addEventListener() {}, removeEventListener() {} }
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const module = { exports: {} as { languageOptions?: (s: string[], i: string) => { id: string; label: string }[] } }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, localStorage: storage, Intl, console })
const languageOptions = module.exports.languageOptions!

// R1 do #590 (D507, D70): o uso mensal zero tem forma natural nos 6 idiomas; os nomes da lista de idiomas do
// reconhecimento acompanham o idioma do app com a tela aberta (só os códigos ficam guardados).

test('R1-01: uso mensal zero sai na forma natural, sem "0", nos 6 idiomas; o dual árabe é próprio', () => {
  for (const idioma of APP_LANGUAGES) {
    const zero = traduzirPlural(idioma, 'voz.ajustes.esteMes', 0, { neste: 'X' })
    assert.doesNotMatch(zero, /0/, `${idioma}: ${zero}`)
    assert.notEqual(zero, traduzirPlural(idioma, 'voz.ajustes.esteMes', 5, { neste: 'X' }), idioma)
  }
  assert.equal(traduzirPlural('pt-BR', 'voz.ajustes.esteMes', 0, { neste: 'neste Mac' }), 'Este mês, neste Mac: nenhum minuto de uso')
  assert.equal(traduzirPlural('en', 'voz.ajustes.esteMes', 0, { neste: 'on this Mac' }), 'This month, on this Mac: no minutes used')
  assert.equal(traduzirPlural('en', 'voz.ajustes.esteMes', 1, { neste: 'on this Mac' }), 'This month, on this Mac: about 1 minute')
  const ar = (n: number) => traduzirPlural('ar-AE', 'voz.ajustes.esteMes', n, { neste: 'X' })
  assert.notEqual(ar(2), ar(1))
  assert.match(ar(2), /دقيقتين/)
})

test('R1-02: os nomes dos idiomas saem no idioma pedido, e a tela não memoiza a lista traduzida', () => {
  const locais = ['en-US', 'pt-BR']
  const pt = languageOptions(locais, 'pt-BR').map((o) => o.label)
  const en = languageOptions(locais, 'en').map((o) => o.label)
  assert.notDeepEqual(pt, en)
  assert.equal(en[0], 'System default')
  const fonte = readFileSync(new URL('./VozPage.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /const idiomas = voice\.languageOptions\(locais, idiomaApp\)/)
  assert.doesNotMatch(fonte, /useMemo\(\(\) => voice\.languageOptions/)
})
