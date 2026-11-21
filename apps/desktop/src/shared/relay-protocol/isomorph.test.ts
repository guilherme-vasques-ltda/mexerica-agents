import test from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

// O pacote é isomórfico: estes arquivos rodam no navegador sem Node. `backend-node.ts`
// é a exceção proposital (só o processo principal do Electron o importa, via calço).
const ISOMORPHIC = ['backend.ts', 'backend-noble.ts', 'ed25519-strict.ts', 'protocol.ts', 'channel.ts', 'relay-socket.ts']
const NODE_IMPORT = /(?:from|import|require)\s*\(?\s*['"]node:/
const BUFFER_USE = /\bBuffer\s*[.(]/
const NODE_BACKEND_IMPORT = /from\s+['"][^'"]*backend-node/

test('protocolo compartilhado não importa node:* nem usa Buffer', () => {
  for (const file of ISOMORPHIC) {
    const src = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
    assert.ok(!NODE_IMPORT.test(src), `${file} não pode importar node:*`)
    assert.ok(!BUFFER_USE.test(src), `${file} não pode usar Buffer`)
  }
})

test('só o calço node liga o backend node:crypto', () => {
  const src = readFileSync(new URL('./backend-node.ts', import.meta.url), 'utf8')
  assert.match(src, /from 'node:crypto'/)
  for (const file of ISOMORPHIC) {
    const other = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
    assert.ok(!NODE_BACKEND_IMPORT.test(other), `${file} não pode importar o backend node`)
  }
})

// ---- grafo de imports TRANSITIVO ----

const IMPORT = /(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)['"]([^'"\s]+)['"]/g
const here = dirname(fileURLToPath(import.meta.url))

/** Arquivos `.ts` alcançáveis por imports relativos a partir de `roots`, e os especificadores nus (pacotes/node:). */
function graph(roots: string[]) {
  const files = new Set<string>()
  const bare = new Set<string>()
  const queue = roots.map((r) => join(here, r))
  while (queue.length) {
    const file = queue.pop()!
    if (files.has(file)) continue
    files.add(file)
    const src = readFileSync(file, 'utf8')
    for (const m of src.matchAll(IMPORT)) {
      const spec = m[1]!
      if (!spec.startsWith('.')) {
        bare.add(spec)
        continue
      }
      const target = join(dirname(file), spec)
      if (existsSync(target) && target.endsWith('.ts')) queue.push(target)
      else if (existsSync(`${target}.ts`)) queue.push(`${target}.ts`)
      else throw new Error(`${file}: import relativo não resolvido: ${spec}`)
    }
  }
  return { files, bare }
}

test('grafo web transitivo: nenhum node:* nem Buffer em nenhum arquivo alcançável', () => {
  const { files, bare } = graph(ISOMORPHIC)
  assert.ok(files.size >= ISOMORPHIC.length)
  for (const spec of bare) assert.ok(!spec.startsWith('node:'), `o grafo web importa ${spec}`)
  for (const file of files) {
    const src = readFileSync(file, 'utf8')
    assert.ok(!BUFFER_USE.test(src), `${file} usa Buffer`)
    assert.ok(!NODE_IMPORT.test(src), `${file} importa node:*`)
  }
})

