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

test('o typecheck E2E da cloud (que importa os calços do desktop) tem "paths" para todo pacote do grafo (checagem estática)', () => {
  const { bare } = graph(['../../main/fork-cloud/crypto.ts', '../../main/fork-cloud/channel.ts', '../../main/fork-cloud/relay-socket.ts'])
  const e2e = JSON.parse(readFileSync(new URL('../../../../../cloud/test/e2e/tsconfig.json', import.meta.url), 'utf8')) as { compilerOptions: { paths?: Record<string, string[]> } }
  const mapped = e2e.compilerOptions.paths ?? {}
  for (const spec of bare) {
    if (spec.startsWith('node:')) continue // tipos do Node: `types: ["node"]` na cloud
    const target = mapped[spec]
    assert.ok(target?.length, `${spec} precisa de "paths" em cloud/test/e2e/tsconfig.json (o job cloud só instala cloud/)`)
    // Estático, sem abrir node_modules (o job desktop não instala cloud/): o destino tem de ser o
    // `.d.ts` do mesmo especificador em cloud/node_modules; que o arquivo exista é o `tsc -p test/e2e` do job cloud.
    assert.equal(target![0], `../../node_modules/${spec.replace(/\.js$/, '.d.ts')}`, `${spec}: destino fora do padrão cloud/node_modules/<pacote>.d.ts`)
  }
})
