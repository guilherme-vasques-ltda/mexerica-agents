import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { tradutor } from './fork-i18n/index.ts'
import { islandLabel } from '../../shared/island.ts'

// R1-01 e R1-02 do #637 (D454, D507, D70): o rótulo do entalhe é uma chave, e quem desenha passa por t();
// "Sessão" e "Semanal" saem do catálogo no idioma de quem desenha (nome de modelo fica como veio).
const raiz = new URL('../../', import.meta.url).pathname
const bundle = await build({
  stdin: { contents: "export { windowTitle } from './renderer/views/usage/usage-model'", resolveDir: raiz, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  plugins: [{ name: 'tema', setup(b) {
    b.onResolve({ filter: /\/lib\/theme$/ }, () => ({ path: 'x', namespace: 'tema' }))
    b.onLoad({ filter: /.*/, namespace: 'tema' }, () => ({ contents: 'export const isDark = () => false', loader: 'js' }))
  } }],
})
const module = { exports: {} as Record<string, any> }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), console })
const { windowTitle } = module.exports

test('R1-02: as janelas de uso saem no idioma pedido; o nome de modelo fica como veio', () => {
  const janela = (label: string) => ({ label, percent: 10 })
  assert.equal(windowTitle(tradutor('pt-BR'), janela('5-hour')), 'Sessão')
  assert.equal(windowTitle(tradutor('en'), janela('5-hour')), 'Session')
  assert.equal(windowTitle(tradutor('en'), janela('Weekly')), 'Weekly')
  assert.equal(windowTitle(tradutor('pt-BR'), janela('Weekly')), 'Semanal')
  assert.equal(windowTitle(tradutor('ar-AE'), janela('Weekly')), 'أسبوعي')
  assert.equal(windowTitle(tradutor('fr'), janela('Weekly · Opus')), 'Opus')
})

test('R1-01: o rótulo do entalhe é chave e todo uso passa por t()/tr(); trocar o idioma muda o rótulo', () => {
  assert.equal(tradutor('en')(islandLabel('darwin')), 'Show in notch')
  assert.equal(tradutor('pt-BR')(islandLabel('darwin')), 'Mostrar no entalhe')
  assert.equal(tradutor('es')(islandLabel('win32')), 'Mostrar en la parte superior de la pantalla')
  const fontes: string[] = []
  const andar = (d: string) => { for (const n of readdirSync(d)) { const p = join(d, n); if (statSync(p).isDirectory()) andar(p); else if (/\.tsx?$/.test(n) && !n.endsWith('.test.ts')) fontes.push(p) } }
  andar(join(raiz, 'renderer')); andar(join(raiz, 'main'))
  const usos = fontes.flatMap((p) => [...readFileSync(p, 'utf8').matchAll(/(\w+)?\(?islandLabel\(/g)].map((m) => ({ p, antes: m[1] })))
  assert.ok(usos.length >= 2, 'barra do Mac e Ajustes usam o rótulo')
  for (const u of usos) assert.ok(u.antes === 't' || u.antes === 'tr', `${u.p}: islandLabel sem t()`)
})
