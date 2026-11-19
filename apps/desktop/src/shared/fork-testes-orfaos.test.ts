// Guarda: todo *.test.ts em src/ precisa estar coberto pelos globs do script "test".
// Sem isto, teste em pasta nova nunca roda e ninguém percebe (ex.: main/fork-idioma.test.ts).
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

function globParaRegex(glob: string): RegExp {
  const re = glob.split('*').map((p) => p.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*')
  return new RegExp(`^${re}$`)
}

function testes(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name)
    if (e.isDirectory()) return e.name === 'node_modules' ? [] : testes(p)
    return e.name.endsWith('.test.ts') ? [relative(desktop, p).split('\\').join('/')] : []
  })
}

test('todo arquivo *.test.ts de src/ é executado pelo npm test', () => {
  const script: string = JSON.parse(readFileSync(join(desktop, 'package.json'), 'utf8')).scripts.test
  const globs = script.split(/\s+/).filter((a) => a.startsWith('src/') && a.endsWith('.test.ts')).map(globParaRegex)
  assert.ok(globs.length > 5, 'não achei os globs no script test')
  const orfaos = testes(join(desktop, 'src')).filter((f) => !globs.some((g) => g.test(f)))
  assert.deepEqual(orfaos, [], `testes fora dos globs do npm test: ${orfaos.join(', ')}`)
})

test('o guarda detecta um órfão', () => {
  const g = globParaRegex('src/shared/*.test.ts')
  assert.ok(g.test('src/shared/a.test.ts'))
  assert.ok(!g.test('src/shared/sub/a.test.ts'))
  assert.ok(!g.test('src/main/a.test.ts'))
})
