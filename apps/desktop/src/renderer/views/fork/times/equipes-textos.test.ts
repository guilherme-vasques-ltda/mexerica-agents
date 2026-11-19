// DON-835 (06/11/2026): a tela que a barra chama de Equipes abre com o título "Equipes" e o botão "Nova equipe".
// Só textos: RPCs, eventos, arquivos e classes continuam "times". Renderiza a TimesScreenView real, com o resto isolado.
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'
import * as fraseMod from '../../../lib/fork-i18n/frase.ts'
import * as i18nMod from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'

const real = createRequire(import.meta.url)
/** Qualquer import não previsto vira uma função vazia (componente que não desenha nada). */
const stub = (): unknown => new Proxy({}, { get: (_t, key) => key === '__esModule' ? true : () => null })

function load(file: string, deps: Record<string, unknown>): Record<string, unknown> {
  const source = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
  const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2023, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } })
  const exports: Record<string, unknown> = {}
  runInNewContext(outputText, { exports, require: (path: string) => path === 'react' || path === 'react/jsx-runtime' ? real(path) : deps[path] ?? stub() })
  return exports
}

/** D454 (fatia 9c): `parts.tsx` com o catálogo de verdade, no idioma pedido. */
const partsEm = (idioma: AppLanguage) => load('parts.tsx', {
  '../../../lib/fork-i18n/estado': { useIdioma: () => idioma, superficieIdioma: i18nMod.superficieIdioma },
  '../../../lib/fork-i18n/frase': fraseMod,
})

test('a tela abre com o título Equipes e o botão Nova equipe, no idioma escolhido e com lang/dir na raiz (D454)', () => {
  for (const [idioma, titulo, novo, solto, dir] of [
    ['pt-BR', 'Equipes', 'Nova equipe', 'Sem equipe', 'ltr'],
    ['en', 'Teams', 'New team', 'No team', 'ltr'],
    ['ar-AE', 'الفرق', 'فريق جديد', 'بلا فريق', 'rtl'],
  ] as const) {
    const parts = partsEm(idioma)
    const { TimesScreenView } = load('TimesView.tsx', {
      './parts': parts,
      './navigation': { timesScreen: { spaceId: null, computerId: null, fallbackComputerId: null, close: () => {} } },
      '../../../store/context': { useApp: () => ({ store: () => undefined, selectedStore: null }) },
      '../../../lib/fork-i18n/frase': fraseMod,
    }) as { TimesScreenView: () => ReturnType<typeof createElement> }
    const html = renderToStaticMarkup(createElement(TimesScreenView))
    assert.match(html, new RegExp(`<div class="times-page idioma-superficie" lang="${idioma}" dir="${dir}">`), idioma)
    assert.match(html, new RegExp(`<h1 class="times-title">${titulo}</h1>`), idioma)
    const { textoDe } = fraseMod
    const { TF } = parts as { TF: Record<string, fraseMod.Frase> }
    assert.equal(textoDe(TF.newTeam!, idioma), novo)
    assert.equal(textoDe(TF.loose!, idioma), solto)
  }
  // O botão do cabeçalho e o do quadro vazio usam esse texto.
  const view = readFileSync(new URL('./TimesView.tsx', import.meta.url), 'utf8')
  assert.equal(view.match(/\{T\.newTeam\}/g)?.length, 2)
})
