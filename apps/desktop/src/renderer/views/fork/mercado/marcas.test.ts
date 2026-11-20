// As logos do catálogo: cada ficha do Hub tem o arquivo da logo no app e cada arquivo tem a origem registrada
// (THIRD_PARTY.md, com o SHA-256 do arquivo baixado). Sem isto, uma ficha nova nasce com o globo ou uma logo sem procedência.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { test } from 'node:test'

const raiz = join(import.meta.dirname, '../../../../../../..')
const pastaMarcas = join(raiz, 'apps/desktop/src/renderer/assets/marcas')
const pastaFichas = join(raiz, 'host/src/fork/conectores/fichas')
const marcasTs = readFileSync(join(raiz, 'apps/desktop/src/renderer/views/marketplace/marcas.ts'), 'utf8')
const thirdParty = readFileSync(join(raiz, 'THIRD_PARTY.md'), 'utf8')
const arquivos = readdirSync(pastaMarcas)
const stem = (f: string) => f.replace(/\.[^.]+$/, '')

/** A chave da logo de cada ficha: o id, ou o nome do arquivo sem `ficha-` na ficha do dono A (o Hub troca a chave dela). */
const chaves = readdirSync(pastaFichas).filter((f) => f.endsWith('.json')).map((f) => {
  const ficha = JSON.parse(readFileSync(join(pastaFichas, f), 'utf8')) as { id: string }
  return f.startsWith('ficha-') ? basename(f, '.json').slice('ficha-'.length) : ficha.id
})

test('toda ficha do Hub tem a logo do fornecedor no app, ligada em marcas.ts', () => {
  for (const chave of chaves) {
    const arquivo = arquivos.find((a) => stem(a) === chave)
    assert.ok(arquivo, `a ficha \`${chave}\` não tem arquivo em assets/marcas`)
    assert.ok(marcasTs.includes(`'../../assets/marcas/${arquivo}'`), `marcas.ts não importa ${arquivo}`)
  }
})

test('todo arquivo de logo tem origem e SHA-256 registrados em THIRD_PARTY.md, e o arquivo não foi alterado', () => {
  for (const arquivo of arquivos) {
    const linha = thirdParty.split('\n').find((l) => l.startsWith('|') && l.includes(`\`${arquivo}\``))
    assert.ok(linha, `${arquivo} não está na tabela de marcas de THIRD_PARTY.md`)
    assert.match(linha, /https?:\/\/|site oficial/, `${arquivo} sem origem`)
    const sha = createHash('sha256').update(readFileSync(join(pastaMarcas, arquivo))).digest('hex')
    assert.ok(linha.includes(sha), `${arquivo}: o SHA-256 da tabela não bate com o arquivo (${sha})`)
  }
})

test('nenhuma logo fica sem ficha, e a chave de marcas.ts existe como arquivo', () => {
  const importadas = [...marcasTs.matchAll(/'\.\.\/\.\.\/assets\/marcas\/([^']+)'/g)].map((m) => m[1])
  assert.deepEqual([...importadas].sort(), [...arquivos].sort(), 'marcas.ts e a pasta têm de ter os mesmos arquivos')
})
