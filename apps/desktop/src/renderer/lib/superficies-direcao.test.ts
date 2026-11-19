// D454: a direção (RTL em ar-AE) de tela migrada. Quatro das últimas cinco revisões caíram em bidi; estas regras
// pegam os casos de custo baixo com o parser do TypeScript, sem renderizar. Só arquivo migrado (superficies-fonte.ts).
//
// R1 superfície em todo retorno: se um componente (função que retorna JSX) aplica a superfície do idioma
//    (`{...superficieIdioma(idioma)}`, ou uma variável que veio dela) em algum retorno, todos os outros retornos de
//    JSX dele também aplicam (`return null` passa). Os retornos antecipados de carregando, erro e "sem computador"
//    ficavam em LTR, fora do `lang`/`dir` do resto da tela. Um retorno que só repassa a outro componente
//    (`return <Foo … />` autofechado) passa SÓ se o Foo for resolvível (do arquivo ou importado por caminho relativo)
//    e todos os retornos dele aplicarem a superfície; senão falha. Callbacks (`.map(...)`) sob um ancestral JSX que
//    já aplica a superfície herdam dela e não são checados.
// R2 hora e telefone em LTR: `formatarTelefone`/`mascararTelefone` e `formatarData` com `hour`/`timeStyle`, escritos
//    direto como filho JSX, ficam dentro de `ltr(...)` ou de um elemento com `dir="ltr"` explícito (`<bdi dir="ltr">`
//    inclusive). Isolar não basta (classe `bidi-isolado`, `<bdi>` sem dir ou com dir="rtl"): em RTL o "+" ainda vai
//    para o fim.
//    Só esses helpers: data por extenso e número têm nomes na língua e não saem invertidos.
// R3 portais (lista, não falha): `<Sheet`/`<FormSheet` não aceitam `language`; ver o teste no fim.
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import test from 'node:test'
import ts from 'typescript'
import { arquivosMigrados, linhaDe } from './superficies-fonte.ts'

// Exceções TEMPORÁRIAS, no formato "caminho relativo a apps/desktop:linha do componente|regra". Cada uma cita o PR aberto
// que ainda altera o arquivo; sai quando o PR entrar e a violação for corrigida lá.
const permitidos = new Map<string, string>([])

const ehFuncao = (n: ts.Node): n is ts.FunctionLikeDeclaration =>
  ts.isFunctionDeclaration(n) || ts.isFunctionExpression(n) || ts.isArrowFunction(n) || ts.isMethodDeclaration(n)

const temJsx = (n: ts.Node): boolean => {
  if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n)) return true
  if (ehFuncao(n)) return false
  return ts.forEachChild(n, (c) => (temJsx(c) ? true : undefined)) ?? false
}

/** Variáveis do arquivo que recebem `superficieIdioma(...)` (também `superficieIdioma(...).lang` não conta: só o objeto). */
function variaveisDeSuperficie(sf: ts.SourceFile): Set<string> {
  const nomes = new Set<string>()
  const ver = (n: ts.Node): void => {
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name) && n.initializer && /\bsuperficieIdioma\(/.test(n.initializer.getText(sf))) nomes.add(n.name.text)
    ts.forEachChild(n, ver)
  }
  ver(sf)
  return nomes
}

/** O retorno aplica a superfície: um spread `{...superficieIdioma(x)}` ou `{...v}` com `v` vinda dela, fora de função interna. */
function aplicaSuperficie(sf: ts.SourceFile, expr: ts.Node, vars: Set<string>): boolean {
  let achou = false
  const ver = (n: ts.Node): void => {
    if (achou) return
    if (ts.isJsxSpreadAttribute(n)) {
      const e = n.expression
      if ((ts.isCallExpression(e) && e.expression.getText(sf) === 'superficieIdioma') || (ts.isIdentifier(e) && vars.has(e.text))) { achou = true; return }
    }
    if (ehFuncao(n)) return
    ts.forEachChild(n, ver)
  }
  ver(expr)
  return achou
}

const cacheSf = new Map<string, ts.SourceFile | null>()
function lerSf(abs: string): ts.SourceFile | null {
  if (!cacheSf.has(abs)) cacheSf.set(abs, existsSync(abs) ? ts.createSourceFile(abs, readFileSync(abs, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX) : null)
  return cacheSf.get(abs)!
}

/** A função do componente `nome` declarada no arquivo (`function Nome`, `const Nome = () =>`, `const Nome = memo(function …)`). */
function acharComponente(sf: ts.SourceFile, nome: string): ts.FunctionLikeDeclaration | null {
  let achada: ts.FunctionLikeDeclaration | null = null
  const ver = (n: ts.Node): void => {
    if (achada) return
    if (ts.isFunctionDeclaration(n) && n.name?.text === nome) { achada = n; return }
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name) && n.name.text === nome && n.initializer) {
      const i = ts.isCallExpression(n.initializer) ? n.initializer.arguments[0] : n.initializer
      if (i && (ts.isArrowFunction(i) || ts.isFunctionExpression(i))) { achada = i; return }
    }
    ts.forEachChild(n, ver)
  }
  ver(sf)
  return achada
}

/** De onde o arquivo importa `nome` por caminho relativo: o arquivo e o nome exportado lá. */
function importadoDe(sf: ts.SourceFile, nome: string): { abs: string; nome: string } | null {
  for (const st of sf.statements) {
    if (!ts.isImportDeclaration(st) || !ts.isStringLiteral(st.moduleSpecifier) || !st.moduleSpecifier.text.startsWith('.')) continue
    const el = st.importClause?.namedBindings
    if (!el || !ts.isNamedImports(el)) continue
    const e = el.elements.find((x) => x.name.text === nome)
    if (!e) continue
    const base = resolve(dirname(sf.fileName), st.moduleSpecifier.text)
    const abs = [`${base}.tsx`, resolve(base, 'index.tsx')].find((c) => existsSync(c))
    return abs ? { abs, nome: (e.propertyName ?? e.name).text } : null
  }
  return null
}

/** O componente `nome` aplica a superfície em todo retorno de JSX (ou repassa a outro que aplique). Não resolvido: não aplica. */
function componenteAplica(sf: ts.SourceFile, nome: string, visitados: Set<string>): boolean {
  const chave = `${sf.fileName}#${nome}`
  if (visitados.has(chave)) return false // ciclo: o componente já está no caminho desta resolução
  // `visitados` é o CAMINHO até aqui (uma cópia por ramo): retornos irmãos que levam ao mesmo destino não são ciclo.
  const caminho = new Set(visitados).add(chave)
  const f = acharComponente(sf, nome)
  if (f) {
    const retornos = retornosDeJsx(f)
    const vars = variaveisDeSuperficie(sf)
    return retornos.length > 0 && retornos.every((r) => aplicaSuperficie(sf, r, vars) || delegaAComponente(sf, r, caminho))
  }
  const de = importadoDe(sf, nome)
  const alvo = de ? lerSf(de.abs) : null
  return !!de && !!alvo && componenteAplica(alvo, de.nome, caminho)
}

/** O retorno só repassa a outro componente (`return <Foo … />`) e esse componente aplica a superfície. */
function delegaAComponente(sf: ts.SourceFile, expr: ts.Node, visitados = new Set<string>()): boolean {
  const e = ts.isParenthesizedExpression(expr) ? expr.expression : expr
  if (!ts.isJsxSelfClosingElement(e)) return false
  const tag = e.tagName.getText(sf)
  return /^[A-Z][A-Za-z0-9]*$/.test(tag) && componenteAplica(sf, tag, visitados)
}

/** Callback sob um ancestral JSX que já aplica a superfície por spread: herda `lang`/`dir`, não é uma superfície própria. */
function herdaSuperficie(sf: ts.SourceFile, f: ts.Node, vars: Set<string>): boolean {
  if (ts.isFunctionDeclaration(f) || !f.parent || !ts.isCallExpression(f.parent)) return false
  for (let p: ts.Node | undefined = f.parent; p; p = p.parent) {
    if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
      const abertura = ts.isJsxElement(p) ? p.openingElement : p
      if (abertura.attributes.properties.some((a) => ts.isJsxSpreadAttribute(a) && ((ts.isCallExpression(a.expression) && a.expression.expression.getText(sf) === 'superficieIdioma') || (ts.isIdentifier(a.expression) && vars.has(a.expression.text))))) return true
    }
  }
  return false
}

/** Os `return` de JSX do corpo da função (sem os das funções internas) e, em seta de expressão, o próprio corpo. */
function retornosDeJsx(f: ts.FunctionLikeDeclaration): ts.Node[] {
  if (!f.body) return []
  if (!ts.isBlock(f.body)) return temJsx(f.body) ? [f.body] : []
  const saida: ts.Node[] = []
  const ver = (n: ts.Node): void => {
    if (ehFuncao(n)) return
    if (ts.isReturnStatement(n) && n.expression && temJsx(n.expression)) saida.push(n.expression)
    ts.forEachChild(n, ver)
  }
  ver(f.body)
  return saida
}

function falhasR1(rel: string, sf: ts.SourceFile): string[] {
  const falhas: string[] = []
  const vars = variaveisDeSuperficie(sf)
  const ver = (n: ts.Node): void => {
    if (ehFuncao(n)) {
      const retornos = retornosDeJsx(n)
      if (retornos.length > 1 && !herdaSuperficie(sf, n, vars)) {
        const com = retornos.filter((r) => aplicaSuperficie(sf, r, vars))
        if (com.length > 0 && com.length < retornos.length) {
          for (const r of retornos) {
            if (com.includes(r) || delegaAComponente(sf, r) || permitidos.has(`${rel}:${linhaDe(sf, n)}|R1`)) continue
            falhas.push(`${rel}:${linhaDe(sf, r)}: retorno de JSX sem a superfície do idioma, e outro retorno do mesmo componente tem; aplique {...superficieIdioma(idioma)} aqui também`)
          }
        }
      }
    }
    ts.forEachChild(n, ver)
  }
  ver(sf)
  return falhas
}

const TELEFONE = new Set(['formatarTelefone', 'mascararTelefone'])

/** Hora: `formatarData(..., { hour… })` ou `{ timeStyle… }`. */
const ehHora = (sf: ts.SourceFile, c: ts.CallExpression): boolean =>
  c.expression.getText(sf) === 'formatarData' && c.arguments.length >= 3 && /\b(hour|timeStyle)\b/.test(c.arguments[2]!.getText(sf))

/** O ancestral mais próximo que fixa a direção (`dir`) a fixa em LTR: `dir="ltr"` explícito, em qualquer elemento (`<bdi dir="ltr">`). */
function emLtr(sf: ts.SourceFile, node: ts.Node): boolean {
  for (let p: ts.Node | undefined = node.parent; p && !ehFuncao(p); p = p.parent) {
    if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
      const abertura = ts.isJsxElement(p) ? p.openingElement : p
      const dir = abertura.attributes.properties.find((a): a is ts.JsxAttribute => ts.isJsxAttribute(a) && a.name.getText(sf) === 'dir')
      if (dir) return dir.initializer?.getText(sf).replace(/['"{}\s]/g, '') === 'ltr'
    }
  }
  return false
}

function falhasR2(rel: string, sf: ts.SourceFile): string[] {
  const falhas: string[] = []
  const ver = (n: ts.Node): void => {
    // Filho JSX escrito em `{…}`: a expressão é a chamada direta (não está dentro de `ltr(...)`, que seria outra chamada).
    if (ts.isJsxExpression(n) && n.expression && (ts.isJsxElement(n.parent) || ts.isJsxFragment(n.parent))) {
      const e = ts.isParenthesizedExpression(n.expression) ? n.expression.expression : n.expression
      if (ts.isCallExpression(e)) {
        const nome = e.expression.getText(sf)
        const tipo = TELEFONE.has(nome) ? 'telefone' : ehHora(sf, e) ? 'hora' : null
        if (tipo && !emLtr(sf, n) && !permitidos.has(`${rel}:${linhaDe(sf, n)}|R2`)) {
          falhas.push(`${rel}:${linhaDe(sf, n)}: ${tipo} (${nome}) escrito direto no JSX; envolva em ltr(...) de @shared/bidi ou em um elemento dir="ltr"`)
        }
      }
    }
    ts.forEachChild(n, ver)
  }
  ver(sf)
  return falhas
}

test('R1: retorno de JSX sem a superfície do idioma quando outro retorno do componente tem (D454)', () => {
  const lista = arquivosMigrados()
  assert.ok(lista.length > 50, `só ${lista.length} arquivos migrados achados`)
  const falhas = lista.flatMap(({ rel, sf }) => falhasR1(rel, sf))
  assert.deepEqual(falhas, [], `\n${falhas.join('\n')}\n`)
})

test('R2: hora e telefone escritos direto no JSX ficam em LTR (D454)', () => {
  const falhas = arquivosMigrados().flatMap(({ rel, sf }) => falhasR2(rel, sf))
  assert.deepEqual(falhas, [], `\n${falhas.join('\n')}\n`)
})

test('as regras pegam o caso e deixam passar o correto', () => {
  const sf = (codigo: string) => ts.createSourceFile('x.tsx', codigo, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const r1 = (codigo: string) => falhasR1('x.tsx', sf(codigo))
  const r2 = (codigo: string) => falhasR2('x.tsx', sf(codigo))
  const base = 'const s = superficieIdioma(i);'
  // R1: o retorno antecipado sem superfície falha; `return null` e retornos todos com a superfície passam.
  assert.equal(r1(`function A() { ${base} if (x) return <p>carregando</p>; return <div {...s}>ok</div> }`).length, 1)
  assert.equal(r1("function A() { if (x) return null; return <div {...superficieIdioma(i)}>ok</div> }").length, 0)
  assert.equal(r1(`function A() { ${base} if (x) return <p {...s}>c</p>; return <div {...s}>ok</div> }`).length, 0)
  assert.equal(r1("function A() { if (x) return <p>c</p>; return <div>ok</div> }").length, 0)
  // R1-01: o autofechado só passa se o componente de destino for resolvível e aplicar a superfície.
  const comDestino = (destino: string) => `${destino}\nfunction A() { ${base} if (x) return <Alvo a={1} />; return <div {...s}>ok</div> }`
  assert.equal(r1(comDestino("function Alvo() { return <p>sem superfície</p> }")).length, 1)
  assert.equal(r1(comDestino("function Alvo() { return <p {...superficieIdioma(i)}>com superfície</p> }")).length, 0)
  assert.equal(r1(comDestino("const Alvo = memo(function Alvo() { if (y) return <i {...superficieIdioma(i)}>c</i>; return <b {...superficieIdioma(i)}>d</b> })")).length, 0)
  // Alvo com um retorno sem superfície não vale como destino (e ele próprio falha no R1): 2 achados.
  assert.equal(r1(comDestino("function Alvo() { if (y) return <i>c</i>; return <b {...superficieIdioma(i)}>d</b> }")).length, 2)
  assert.equal(r1(comDestino("import { Alvo } from './nao-existe'")).length, 1)
  assert.equal(r1(`function A() { ${base} if (x) return <Text children={t('carregando')} />; return <div {...s}>ok</div> }`).length, 1)
  assert.equal(r1(comDestino("function Alvo() { return <Outro /> }\nfunction Outro() { return <p {...superficieIdioma(i)}>c</p> }")).length, 0)
  assert.equal(r1(`function A() { ${base} if (x) return <Cartao><p>c</p></Cartao>; return <div {...s}>ok</div> }`).length, 1)
  // R2-01: dois retornos que levam ao mesmo destino com superfície não são ciclo; ciclo real continua falhando.
  const folha = "function Folha() { return <p {...superficieIdioma(i)}>ok</p> }"
  assert.equal(r1(comDestino(`${folha}\nfunction Alvo() { if (y) return <Folha />; return <Folha /> }`)).length, 0)
  assert.equal(r1(comDestino(`${folha}\nfunction Alvo() { if (y) return <Folha />; return <Outro /> }\nfunction Outro() { return <Folha /> }`)).length, 0)
  assert.equal(r1(comDestino("function Alvo() { return <Outro /> }\nfunction Outro() { return <Alvo /> }")).length, 1)
  // Ciclo dentro do próprio destino: o `<Alvo />` recursivo também falha no R1 do Alvo (2 achados: o do A e o do Alvo).
  assert.equal(r1(comDestino("function Alvo() { if (y) return <Alvo />; return <p {...superficieIdioma(i)}>c</p> }")).length, 2)
  assert.equal(r1(comDestino(`${folha}\nfunction Alvo() { if (y) return <Folha />; return <Outro /> }\nfunction Outro() { return <Alvo /> }`)).length, 1) // ciclo Alvo → Outro → Alvo, mesmo com a Folha ao lado
  // R1-03: callback sob um ancestral que já aplica a superfície herda; sob um ancestral sem ela, não.
  assert.equal(r1(`function A() { ${base} return <ul {...s}>{xs.map((x) => { if (x) return <li>a</li>; return <li {...s}>b</li> })}</ul> }`).length, 0)
  assert.equal(r1(`function A() { ${base} return <ul>{xs.map((x) => { if (x) return <li>a</li>; return <li {...s}>b</li> })}</ul> }`).length, 1)
  // R2: telefone e hora soltos falham; só ltr(...) ou dir="ltr" explícito passam.
  assert.equal(r2("const A = () => <span>{formatarTelefone(p)}</span>").length, 1)
  assert.equal(r2("const A = () => <span>{formatarData(i, d, { hour: '2-digit' })}</span>").length, 1)
  assert.equal(r2("const A = () => <span>{ltr(formatarTelefone(p))}</span>").length, 0)
  assert.equal(r2("const A = () => <span dir=\"ltr\">{formatarTelefone(p)}</span>").length, 0)
  assert.equal(r2("const A = () => <bdi dir=\"ltr\">{mascararTelefone(p)}</bdi>").length, 0)
  assert.equal(r2("const A = () => <span dir=\"rtl\"><span dir=\"ltr\">{formatarTelefone(p)}</span></span>").length, 0)
  // R1-02: isolar não fixa LTR.
  assert.equal(r2("const A = () => <span className=\"bidi-isolado\">{mascararTelefone(p)}</span>").length, 1)
  assert.equal(r2("const A = () => <bdi>{formatarTelefone(p)}</bdi>").length, 1)
  assert.equal(r2("const A = () => <bdi dir=\"rtl\">{formatarTelefone(p)}</bdi>").length, 1)
  assert.equal(r2("const A = () => <span dir=\"ltr\"><span dir=\"rtl\">{formatarTelefone(p)}</span></span>").length, 1)
  assert.equal(r2("const A = () => <span>{formatarData(i, d, { month: 'long' })}</span>").length, 0)
})
