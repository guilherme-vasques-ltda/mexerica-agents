// Guarda do índice docs/security/regressoes.md: todo id de achado de segurança
// citado no código tem linha no índice, e cada linha aponta teste que existe e cita o id.
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const raiz = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..')
const RAIZES = ['host', 'apps/desktop/src', 'apps/desktop/e2e-web', 'apps/screen-linux', 'cloud/src', 'cloud/test', 'relay/src', 'relay/test']
const IGNORAR = new Set(['node_modules', 'target', '.git'])
const ID = /\b(SEG-[A-Z0-9]+(?:-[A-Z0-9]+)*|CF-[0-9]+)\b/g
const ARQUIVO = /\.(rs|tsx?|mjs)$/
const ehArquivoDeTeste = (p: string) =>
  /(^|\/)(tests?|testes?|testes_[^/]*|e2e-web)\//.test(p) ||
  /(^|\/)(tests?|testes?)(_[^/]*)?\.rs$/.test(p) ||
  /(_tests?|_testes?)\.rs$/.test(p) ||
  /\.(test|spec)\.tsx?$/.test(p) ||
  /\/tests?_[^/]*$/.test(p)
// Em Rust, teste também mora em `#[cfg(test)] mod tests` dentro do arquivo de produção:
// uma citação dentro desse mod conta como teste (ver faixasDeTeste).
// Só `#[cfg(test)] mod x {`: `#[cfg(test)]` também marca itens de produção (fn, use, campos).
const MARCA_CFG_TEST = /^([ \t]*)#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/gm

// Faixas [início, fim) de `#[cfg(test)] mod x { … }`. O fim é a primeira linha seguinte que
// fecha com `}` na mesma indentação da marca (o rustfmt do CI garante esse formato).
// Strings (inclusive raw), comentários (inclusive blocos aninhados) e `'"'` são apagados
// (mantendo o tamanho) para que um `}` dentro deles não pareça o fechamento do mod.
function mascarar(t: string): string {
  const out = t.split('')
  const apagar = (de: number, ate: number) => {
    for (let k = de; k < ate; k++) if (out[k] !== '\n') out[k] = ' '
  }
  let i = 0
  while (i < t.length) {
    const c = t[i]
    let fim = -1
    if (c === '/' && t[i + 1] === '/') {
      const nl = t.indexOf('\n', i)
      fim = nl < 0 ? t.length : nl
    } else if (c === '/' && t[i + 1] === '*') {
      let prof = 1
      fim = i + 2
      while (fim < t.length && prof > 0) {
        if (t[fim] === '/' && t[fim + 1] === '*') (prof++, (fim += 2))
        else if (t[fim] === '*' && t[fim + 1] === '/') (prof--, (fim += 2))
        else fim++
      }
    } else if ((c === 'r' || ((c === 'b' || c === 'c') && t[i + 1] === 'r')) && !/[A-Za-z0-9_]/.test(t[i - 1] ?? ' ')) {
      const ini = c === 'r' ? i + 1 : i + 2
      let h = ini
      while (t[h] === '#') h++
      if (t[h] === '"') {
        const f = t.indexOf(`"${t.slice(ini, h)}`, h + 1)
        fim = f < 0 ? t.length : f + 1 + (h - ini)
      }
    } else if (c === '"') {
      fim = i + 1
      while (fim < t.length && t[fim] !== '"') fim += t[fim] === '\\' ? 2 : 1
      fim = Math.min(fim + 1, t.length)
    } else if (c === "'" && /^'(?:\\.|[^'\\\n])'/.test(t.slice(i, i + 6))) {
      fim = i + /^'(?:\\.|[^'\\\n])'/.exec(t.slice(i, i + 6))![0].length
    }
    if (fim > i) (apagar(i, fim), (i = fim))
    else i++
  }
  return out.join('')
}

function faixasDeTeste(original: string): Array<[number, number]> {
  const texto = mascarar(original)
  return [...texto.matchAll(MARCA_CFG_TEST)].map((m) => {
    const fecha = new RegExp(`^${m[1]}\\}`, 'm')
    const resto = texto.slice(m.index + m[0].length)
    const f = resto.search(fecha)
    return [m.index, f < 0 ? texto.length : m.index + m[0].length + f] as [number, number]
  })
}

function arquivos(dir: string): string[] {
  if (!existsSync(join(raiz, dir))) return []
  return readdirSync(join(raiz, dir), { withFileTypes: true }).flatMap((e) => {
    const rel = `${dir}/${e.name}`
    if (e.isDirectory()) return IGNORAR.has(e.name) ? [] : arquivos(rel)
    return ARQUIVO.test(e.name) ? [rel] : []
  })
}

function idsNoCodigo() {
  const mapa = new Map<string, { testes: Set<string>; codigo: Set<string> }>()
  for (const arq of RAIZES.flatMap(arquivos)) {
    const texto = readFileSync(join(raiz, arq), 'utf8')
    const arquivoDeTeste = ehArquivoDeTeste(arq)
    const faixas = arq.endsWith('.rs') ? faixasDeTeste(texto) : []
    for (const m of texto.matchAll(ID)) {
      const e = mapa.get(m[1]) ?? { testes: new Set(), codigo: new Set() }
      const emTeste = arquivoDeTeste || faixas.some(([a, b]) => m.index >= a && m.index < b)
      ;(emTeste ? e.testes : e.codigo).add(arq)
      mapa.set(m[1], e)
    }
  }
  return mapa
}

function linhasDoIndice() {
  const md = readFileSync(join(raiz, 'docs/security/regressoes.md'), 'utf8')
  const linhas = new Map<string, { testes: string[]; codigo: string[] }>()
  const arquivosDe = (c: string) => [...c.matchAll(/`([^`]+)`/g)].map((m) => m[1])
  for (const l of md.split('\n')) {
    const cols = l.split('|').map((c) => c.trim())
    if (!/^(SEG-|CF-)/.test(cols[1] ?? '')) continue
    assert.ok(cols.length >= 5, `linha malformada no índice (esperadas 3 colunas): ${l}`)
    assert.ok(!linhas.has(cols[1]), `id duplicado no índice: ${cols[1]}`)
    const testes = cols[2] === 'lacuna' ? [] : arquivosDe(cols[2])
    const codigo = cols[3] === '—' ? [] : arquivosDe(cols[3])
    assert.ok(cols[2] === 'lacuna' || testes.length > 0, `${cols[1]}: coluna de testes malformada (use arquivos entre crases ou "lacuna")`)
    assert.ok(cols[3] === '—' || codigo.length > 0, `${cols[1]}: coluna de código malformada (use arquivos entre crases ou "—")`)
    linhas.set(cols[1], { testes, codigo })
  }
  return linhas
}

test('todo id de achado citado no código está no índice e vice-versa', { skip: 'audita o índice interno de regressões de segurança (docs/security/regressoes.md), que fica privado' }, () => {
  const codigo = idsNoCodigo()
  const indice = linhasDoIndice()
  assert.ok(codigo.size > 10, 'não achei ids no código; o scanner quebrou?')
  assert.deepEqual([...codigo.keys()].filter((i) => !indice.has(i)).sort(), [], 'ids sem linha em docs/security/regressoes.md')
  assert.deepEqual([...indice.keys()].filter((i) => !codigo.has(i)).sort(), [], 'linhas do índice cujo id não existe mais no código')
})

test('cada linha cita testes e código que existem e contêm o id; lacuna não tem teste', { skip: 'audita o índice interno de regressões de segurança (docs/security/regressoes.md), que fica privado' }, () => {
  const codigo = idsNoCodigo()
  for (const [id, { testes, codigo: citados }] of linhasDoIndice()) {
    const reais = codigo.get(id)?.testes ?? new Set<string>()
    const prod = codigo.get(id)?.codigo ?? new Set<string>()
    assert.deepEqual([...prod].sort(), [...citados].sort(), `${id}: coluna "Código que o cita" desatualizada`)
    if (testes.length === 0) {
      assert.equal(reais.size, 0, `${id} marcado lacuna, mas já tem teste: ${[...reais].join(', ')}`)
      continue
    }
    for (const t of testes) {
      assert.ok(existsSync(join(raiz, t)), `${id}: teste inexistente ${t}`)
      assert.ok(reais.has(t), `${id}: ${t} não cita o id`)
    }
    assert.deepEqual([...reais].filter((t) => !testes.includes(t)).sort(), [], `${id}: testes que citam o id e faltam na linha`)
  }
})

test('faixasDeTeste: produção depois do mod de teste não conta como teste', () => {
  const rs = ['fn prod() {}', '', '#[cfg(test)]', 'mod tests {', '    fn t() {}', '}', '', 'fn depois() {}', '#[cfg(test)]', 'fn helper() {}'].join('\n')
  const faixas = faixasDeTeste(rs)
  assert.equal(faixas.length, 1)
  const dentro = (trecho: string) => faixas.some(([a, b]) => rs.indexOf(trecho) >= a && rs.indexOf(trecho) < b)
  assert.ok(dentro('fn t()'))
  assert.ok(!dentro('fn prod()'))
  assert.ok(!dentro('fn depois()'))
  assert.ok(!dentro('fn helper()'))
})

test('faixasDeTeste: "}" em raw string, string e comentário não encerra o mod', () => {
  const rs = ['#[cfg(test)]', 'mod tests {', '    const A: &str = r#"', '}', '"#;', '    const B: &str = "x\\"', '}', '";', '    // }', '    fn t() {}', '}', 'fn prod() {}'].join('\n')
  const faixas = faixasDeTeste(rs)
  const dentro = (trecho: string) => faixas.some(([a, b]) => rs.indexOf(trecho) >= a && rs.indexOf(trecho) < b)
  assert.ok(dentro('fn t()'))
  assert.ok(!dentro('fn prod()'))
})

test('faixasDeTeste: comentário de bloco aninhado não encerra o mod', () => {
  const rs = ['#[cfg(test)]', 'mod tests {', '    /* a /* b', '    */ c', '}', '    */', '    fn t() {}', '}', 'fn prod() {}'].join('\n')
  const faixas = faixasDeTeste(rs)
  const dentro = (trecho: string) => faixas.some(([a, b]) => rs.indexOf(trecho) >= a && rs.indexOf(trecho) < b)
  assert.ok(dentro('fn t()'))
  assert.ok(!dentro('fn prod()'))
})

test('faixasDeTeste: raw strings br/cr e com muitos # não encerram o mod', () => {
  const longo = '#'.repeat(50)
  const rs = ['#[cfg(test)]', 'mod tests {', '    const A: &[u8] = br#"', '}', '"#;', `    const B: &str = r${longo}"`, '}', `"${longo};`, '    const C: &CStr = cr"', '}', '";', '    fn t() {}', '}', 'fn prod() {}'].join('\n')
  const faixas = faixasDeTeste(rs)
  const dentro = (trecho: string) => faixas.some(([a, b]) => rs.indexOf(trecho) >= a && rs.indexOf(trecho) < b)
  assert.ok(dentro('fn t()'))
  assert.ok(!dentro('fn prod()'))
})
