// DON-903, gate de idiomas (docs/guides/idiomas.md). Roda dentro do `npm test` do desktop
// (src/shared/fork-textos-gate.test.ts), no job `desktop` do CI existente.
//
// 1. Catálogos: todo idioma tem exatamente as chaves do pt-BR, sem texto vazio, com os mesmos `{parâmetros}`.
//    Por área (`conferirModulos`): cada módulo registrado em fork-i18n/registro.ts, chave no prefixo do módulo e
//    numa parte só, índice com cada parte uma vez e caminhos antigos (`pt-BR.ts`…) só reexportando.
//    Pasta em fork-i18n/ fora da lista combinada falha. Symlink em src falha. Pasta node_modules dentro de
//    src falha e não é lida. Arquivo .js/.jsx/.mjs/.cjs dentro de src falha, fora os de tools/javascript-legado.json
//    (os que a main já tinha). A lista vem da árvore, então vale sem Git (snapshot do hook, VM da Instabox);
//    com Git, ela não pode crescer em relação ao merge-base (`naoCresceu`). Só encolhe: entrada sem o arquivo falha.
//    O que fica fora da checagem vem do valor exportado de verdade, mais a lista da main já revisada.
//    O arquivo do PR não é lido como texto.
// 2. Texto visível solto: lê cada .ts/.tsx/.mts/.cts do app com o parser do TypeScript (já instalado) e acha texto
//    de produto fora dos catálogos: texto JSX, atributos visíveis e de acessibilidade (aria-label, title,
//    placeholder, alt…), props de texto (`label: '…'`) e frases soltas. Ignora identificadores e dados:
//    imports, tipos, chaves, comparações, `case`, className/id/role e chamadas de log, IPC e armazenamento.
// 3. Legado: o que já existia está em tools/textos-legado.json, por arquivo, texto e número de vezes.
//    Texto que não está lá falha; entrada que sobrou (o texto saiu do código) também falha, para o
//    inventário só encolher (`--encolher` apaga as sobras e nunca acrescenta). Como o JSON pode ser editado no
//    mesmo patch, num PR o inventário é comparado com o texto solto do merge-base (`inventarioDaBase`), não com
//    a ponta da main na hora do teste: só falha entrada ou contagem que o PR acrescentou. A junção com a ponta
//    da main é simulada (`git merge-tree --write-tree`, sem checkout; não altera refs, índice nem arquivos;
//    pode criar objetos soltos): texto solto novo falha; sobra criada
//    pelo PR falha; sobra que a main criou só avisa; conflito em textos-legado.json não reprova.
//
// Uso: node --experimental-strip-types tools/textos-gate.ts [--encolher | --listar [--area <id>] [--json]]
// Exceção pontual de dado (nunca rótulo, texto JSX ou acessibilidade): comentário `textos-ok: <motivo>` junto do
// literal ou da declaração que o contém.
// Lista para tradutores e PRs de troca: --listar [--area <id>] [--json] (só lê).
import { execFileSync } from 'node:child_process'
import { lstatSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, sep } from 'node:path'
import ts from 'typescript'
import { IDIOMAS_CATALOGO, MODULOS, moduloDaChave } from '../src/renderer/lib/fork-i18n/registro.ts'

export interface Achado { arquivo: string; linha: number; texto: string }
/** arquivo → texto → vezes. */
export type Inventario = Record<string, Record<string, number>>

/** Atributos JSX lidos por quem usa o app, inclusive leitores de tela. */
const ATRIBUTOS_VISIVEIS = new Set([
  'aria-label', 'aria-description', 'aria-roledescription', 'aria-valuetext', 'aria-placeholder',
  'title', 'placeholder', 'alt', 'label', 'message', 'detail', 'description', 'hint', 'prompt', 'note',
  'clearLabel', 'emptyText', 'tooltip', 'subtitle', 'caption', 'heading', 'help', 'confirmTitle', 'confirmLabel',
])
/** Props de objeto que levam texto de interface (`{ label: 'Sistema' }`). */
const PROPS_VISIVEIS = new Set([
  'label', 'title', 'message', 'detail', 'description', 'hint', 'placeholder', 'text', 'tooltip', 'subtitle',
  'summary', 'caption', 'heading', 'note', 'help', 'ariaLabel', 'prompt', 'body', 'empty', 'emptyText',
])
/** Chamadas cujo texto é técnico (log, IPC, armazenamento, DOM, testes). Pelo nome final ou pelo objeto. */
const CHAMADAS_TECNICAS = new Set([
  'require', 'getItem', 'setItem', 'removeItem', 'querySelector', 'querySelectorAll', 'closest', 'matches',
  'addEventListener', 'removeEventListener', 'matchMedia', 'getPropertyValue', 'setProperty', 'removeProperty',
  'getAttribute', 'setAttribute', 'hasAttribute', 'removeAttribute', 'createElement', 'createElementNS',
  'pref', 'invoke', 'sendSync', 'handle', 'on', 'once', 'off', 'emit', 'send', 'removeListener',
  'postMessage', 'exposeInMainWorld', 'startsWith', 'endsWith', 'includes', 'split', 'join', 'replace',
  'replaceAll', 'indexOf', 'lastIndexOf', 'padStart', 'padEnd', 'localeCompare', 'test', 'exec', 'match',
  'RegExp', 'Symbol', 'describe', 'it', 'assert', 'debug', 'info', 'warn', 'error', 'log', 'trace',
  'traduzir', 't', 'fetch', 'URL', 'URLSearchParams', 'get', 'has', 'delete', 'append', 'execFile', 'spawn', 'exec',
  'Intl', 'DateTimeFormat', 'NumberFormat', 'toLocaleString', 'toLocaleDateString', 'toLocaleTimeString',
])
const OBJETOS_TECNICOS = new Set(['console', 'ipcMain', 'ipcRenderer', 'localStorage', 'sessionStorage', 'process', 'path', 'fs', 'JSON', 'Reflect', 'Object', 'crypto', 'shell'])

const letra = /\p{L}/u
const acento = /[À-ÖØ-öø-ÿ]/
const palavra3 = /\p{L}{3,}/u

/** Frase de produto: palavra de verdade e espaço, começando em maiúscula, com acento ou com pontuação final. */
function pareceFrase(texto: string): boolean {
  const s = texto.trim()
  if (!palavra3.test(s) || !/\s/.test(s)) return false
  if (/^(?:[a-z]+:\/\/|\/|\.{1,2}\/|#|\$|--|@)/.test(s) || /var\(--|;\s*\S+:|\bpx\b|=>|\{\s*\}/.test(s)) return false
  return /^\p{Lu}/u.test(s) || acento.test(s) || /[.?!…:]$/.test(s)
}

function normalizar(texto: string) {
  return texto.replace(/\s+/g, ' ').trim()
}

function nomeDe(n: ts.Node | undefined): string | null {
  if (!n) return null
  if (ts.isIdentifier(n) || ts.isPrivateIdentifier(n) || ts.isStringLiteral(n)) return n.text
  if (ts.isPropertyAccessExpression(n)) return n.name.text
  return null
}

function raizDe(n: ts.Expression): string | null {
  let e: ts.Expression = n
  while (ts.isPropertyAccessExpression(e) || ts.isCallExpression(e) || ts.isElementAccessExpression(e)) e = e.expression
  return ts.isIdentifier(e) ? e.text : null
}

/** O texto de um literal ou template, com `{}` no lugar das partes calculadas. */
function textoDe(n: ts.Node): string | null {
  if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) return n.text
  if (ts.isTemplateExpression(n)) return n.head.text + n.templateSpans.map((s) => '{}' + s.literal.text).join('')
  return null
}

type Contexto = 'visivel' | 'tecnico' | 'livre'

/** Sobe da posição do literal até decidir se ele é texto de interface, dado técnico ou frase a julgar. */
function contexto(n: ts.Node): Contexto {
  let filho: ts.Node = n
  for (let p = n.parent; p; filho = p, p = p.parent) {
    if (ts.isImportDeclaration(p) || ts.isExportDeclaration(p) || ts.isImportTypeNode(p) || ts.isExternalModuleReference(p)) return 'tecnico'
    if (ts.isTypeNode(p) && !ts.isTypeQueryNode(p)) return 'tecnico'
    if (ts.isLiteralTypeNode(p) || ts.isEnumMember(p) && p.name === filho) return 'tecnico'
    if ((ts.isPropertyAssignment(p) || ts.isPropertyDeclaration(p) || ts.isMethodDeclaration(p) || ts.isPropertySignature(p)) && p.name === filho) return 'tecnico'
    if (ts.isElementAccessExpression(p) && p.argumentExpression === filho) return 'tecnico'
    if (ts.isCaseClause(p) && p.expression === filho) return 'tecnico'
    if (ts.isBinaryExpression(p)) {
      const op = p.operatorToken.kind
      if (op === ts.SyntaxKind.EqualsEqualsEqualsToken || op === ts.SyntaxKind.ExclamationEqualsEqualsToken || op === ts.SyntaxKind.EqualsEqualsToken || op === ts.SyntaxKind.ExclamationEqualsToken || op === ts.SyntaxKind.InKeyword || op === ts.SyntaxKind.InstanceOfKeyword) return 'tecnico'
    }
    if (ts.isJsxAttribute(p)) {
      const nome = p.name.getText()
      return ATRIBUTOS_VISIVEIS.has(nome) ? 'visivel' : 'tecnico'
    }
    if (ts.isJsxExpression(p) && (ts.isJsxElement(p.parent) || ts.isJsxFragment(p.parent))) return 'visivel'
    if (ts.isPropertyAssignment(p) && p.initializer === filho) {
      const nome = nomeDe(p.name)
      if (nome && PROPS_VISIVEIS.has(nome)) return 'visivel'
      return 'livre'
    }
    if (ts.isCallExpression(p) || ts.isNewExpression(p)) {
      if (p.expression === filho) return 'tecnico'
      const nome = nomeDe(p.expression)
      const raiz = raizDe(p.expression)
      if ((nome && CHAMADAS_TECNICAS.has(nome)) || (raiz && OBJETOS_TECNICOS.has(raiz))) return 'tecnico'
      return 'livre'
    }
    if (ts.isTaggedTemplateExpression(p)) return 'tecnico'
    // Ternários, &&, ??, parênteses, arrays e templates passam o contexto de quem está acima.
    if (ts.isJsxExpression(p) || ts.isConditionalExpression(p) || ts.isBinaryExpression(p) || ts.isParenthesizedExpression(p) || ts.isArrayLiteralExpression(p) || ts.isTemplateSpan(p) || ts.isTemplateExpression(p) || ts.isAsExpression(p) || ts.isSatisfiesExpression(p) || ts.isNonNullExpression(p)) continue
    return 'livre'
  }
  return 'livre'
}

/** Os textos visíveis soltos de um arquivo (o que deveria vir de um catálogo). */
export function textosVisiveis(arquivo: string, fonte: string): Achado[] {
  const sf = ts.createSourceFile(arquivo, fonte, ts.ScriptTarget.Latest, true, arquivo.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  // Exceção pontual só para dado em contexto livre (nunca texto JSX, atributo visível ou prop de texto) e só com
  // comentário de verdade `textos-ok: <motivo>` junto do literal ou da declaração que o contém.
  const marcado = (n: ts.Node) => {
    const comentarios: string[] = []
    const ler = (r: ts.CommentRange[] | undefined) => r?.forEach((c) => comentarios.push(fonte.slice(c.pos, c.end)))
    for (let p: ts.Node | undefined = n; p && !ts.isSourceFile(p); p = p.parent) {
      ler(ts.getLeadingCommentRanges(fonte, p.getFullStart()))
      ler(ts.getTrailingCommentRanges(fonte, p.getEnd()))
      if (ts.isStatement(p) || ts.isPropertyAssignment(p) || ts.isClassElement(p)) {
        // Vírgula depois da propriedade: o comentário da linha vem depois dela.
        ler(ts.getTrailingCommentRanges(fonte, fonte[p.getEnd()] === ',' ? p.getEnd() + 1 : p.getEnd()))
        break
      }
    }
    return comentarios.some((c) => /textos-ok:\s*\S.{2,}/.test(c))
  }
  const achados: Achado[] = []
  const anotar = (n: ts.Node, texto: string) => {
    const linha = sf.getLineAndCharacterOfPosition(n.getStart(sf)).line
    achados.push({ arquivo, linha: linha + 1, texto: normalizar(texto) })
  }
  const visitar = (n: ts.Node): void => {
    if (ts.isJsxText(n)) {
      const texto = normalizar(n.text)
      // Entidades (`&nbsp;`) não são texto.
      if (letra.test(texto.replace(/&(?:[a-z]+|#\d+|#x[0-9a-f]+);/gi, ''))) anotar(n, texto)
      return
    }
    const texto = textoDe(n)
    if (texto !== null) {
      const c = contexto(n)
      if (c === 'visivel' ? letra.test(texto) : c === 'livre' && pareceFrase(texto) && !marcado(n)) anotar(n, texto)
      if (ts.isTemplateExpression(n)) n.templateSpans.forEach((s) => visitar(s.expression))
      return
    }
    ts.forEachChild(n, visitar)
  }
  visitar(sf)
  return achados
}

const I18N = 'src/renderer/lib/fork-i18n'
/** Os arquivos de tradução declarados no registro, por idioma: o índice e cada módulo (`pt-BR/whatsapp/numero.ts`). */
export function modulosDeclarados(): string[] {
  return IDIOMAS_CATALOGO.flatMap((i) => [`${I18N}/${i}/index.ts`, ...MODULOS.map((m) => `${I18N}/${i}/${m.modulo}.ts`)])
}
const DECLARADOS = new Set(modulosDeclarados())

/**
 * `candidato`: a árvore conferida. `base`: o código da base lido do Git, que pode ser anterior à divisão por área.
 * Os catálogos planos (`pt-BR.ts`…) só ficam de fora na base, onde eram o catálogo inteiro; no candidato eles
 * são lidos e `conferirModulos` exige que só reexportem.
 */
export type Modo = 'candidato' | 'base'

/**
 * Fora do gate, cada um com motivo: testes e fixtures, a demo do site (dados fictícios), a demo dos avatares
 * (vitrine isolada da biblioteca, não é tela do produto) e os catálogos: só os arquivos declarados no registro
 * (`conferirModulos` valida a forma deles) e, na base, os planos antigos.
 */
export function foraDoGate(caminho: string, modo: Modo = 'candidato', declarados: ReadonlySet<string> = DECLARADOS): boolean {
  return /\.test\.(?:mts|cts|tsx?)$/.test(caminho) || /\.fixture\.(?:mts|cts|ts)$/.test(caminho) || /\.d\.(?:mts|cts|tsx?)$/.test(caminho)
    || caminho.startsWith('src/renderer/demo/')
    || caminho.startsWith('src/renderer/lib/donnabot-avatars/demo/')
    || declarados.has(caminho)
    || (modo === 'base' && /^src\/renderer\/lib\/fork-i18n\/(pt-BR|en|es|fr|zh-CN|ar-AE)\.ts$/.test(caminho))
}

/** Texto de tela: TypeScript, inclusive os módulos `.mts` e `.cts`. */
const EXT_TEXTO = /\.(?:mts|cts|tsx?)$/
/** JavaScript dentro de `src` não é fonte do renderer. Arquivo que a main não tem falha. */
const EXT_JS = /\.(?:js|jsx|mjs|cjs)$/

const semTipo = (e: ts.Expression): ts.Expression => {
  while (ts.isAsExpression(e) || ts.isSatisfiesExpression(e) || ts.isParenthesizedExpression(e)) e = e.expression
  return e
}

const textoLiteral = (e: ts.Expression): string | null => {
  const v = semTipo(e)
  return ts.isStringLiteral(v) || ts.isNoSubstitutionTemplateLiteral(v) ? v.text : null
}

const exportado = (n: ts.VariableStatement) => n.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword) ?? false

/**
 * Só o `export` de um `registro.ts` já revisado (a main). Comentário e variável local não entram.
 * Forma que não seja lista de literais: `null`, e fica valendo só o valor carregado pelo código.
 */
function registroExportado(fonte: string): { idiomas: string[]; modulos: string[] } | null {
  const sf = ts.createSourceFile('registro.ts', fonte, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS)
  let idiomas: string[] | null = null
  let modulos: string[] | null = null
  for (const n of sf.statements) {
    if (!ts.isVariableStatement(n) || !exportado(n)) continue
    for (const d of n.declarationList.declarations) {
      if (!ts.isIdentifier(d.name) || !d.initializer) continue
      const ini = semTipo(d.initializer)
      if (!ts.isArrayLiteralExpression(ini)) continue
      if (d.name.text === 'IDIOMAS_CATALOGO') {
        const nomes = ini.elements.map((e) => ts.isExpression(e) ? textoLiteral(e) : null)
        if (nomes.some((nome) => nome === null)) return null
        idiomas = nomes as string[]
      } else if (d.name.text === 'MODULOS') {
        const nomes: string[] = []
        for (const el of ini.elements) {
          if (!ts.isExpression(el)) return null
          const obj = semTipo(el)
          if (!ts.isObjectLiteralExpression(obj)) return null
          let nome: string | null = null
          let repetido = false
          for (const p of obj.properties) {
            if (!ts.isPropertyAssignment(p)) continue
            const chave = ts.isIdentifier(p.name) || ts.isStringLiteral(p.name) ? p.name.text : null
            if (chave !== 'modulo') continue
            if (nome !== null) repetido = true
            nome = textoLiteral(p.initializer)
          }
          if (repetido || nome === null) return null
          nomes.push(nome)
        }
        modulos = nomes
      }
    }
  }
  if (!idiomas?.length || !modulos?.length) return null
  return { idiomas, modulos }
}

const relDe = (raiz: string, p: string) => relative(raiz, p).split(sep).join('/')

/** Symlink, `node_modules` e JavaScript em `src`, e pasta de `fork-i18n` fora da lista. Não segue symlink nem entra em `node_modules`. */
export function falhasEstruturais(
  raiz: string,
  idiomas: ReadonlySet<string> = new Set(IDIOMAS_CATALOGO),
): string[] {
  const legado = lerJavascriptLegado(raiz)
  const erros: string[] = legado.erro ? [legado.erro] : []
  const src = join(raiz, 'src')
  let topo: ReturnType<typeof lstatSync> | null = null
  try { topo = lstatSync(src) } catch { return erros }
  if (topo.isSymbolicLink()) return ['src: symlink em src']
  const andar = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name)
      const rel = relDe(raiz, p)
      if (e.isSymbolicLink()) { erros.push(`${rel}: symlink em src`); continue }
      if (e.isDirectory()) {
        if (e.name === 'node_modules') erros.push(`${rel}: node_modules dentro de src`)
        else andar(p)
        continue
      }
      if (EXT_JS.test(e.name) && !legado.erro && !legado.arquivos.has(rel)) erros.push(`${rel}: JavaScript em src`)
    }
  }
  andar(src)
  // A lista não vira licença: entrada sem o arquivo na árvore falha (vale sem Git).
  for (const rel of legado.arquivos) {
    let existe = false
    try { existe = lstatSync(join(raiz, rel)).isFile() } catch { /* sem o arquivo */ }
    if (!existe) erros.push(`${rel}: sobra em tools/javascript-legado.json; remova a entrada`)
  }
  try {
    for (const e of readdirSync(join(raiz, I18N), { withFileTypes: true })) {
      if (e.isSymbolicLink()) continue
      if (e.isDirectory() && !idiomas.has(e.name)) erros.push(`${I18N}/${e.name}: pasta que não é um idioma declarado`)
    }
  } catch { /* sem a pasta de idiomas */ }
  return erros
}

export function arquivosDoApp(
  raiz: string,
  modo: Modo = 'candidato',
  declarados: ReadonlySet<string> = DECLARADOS,
): string[] {
  const herdados = lerJavascriptLegado(raiz).arquivos
  const saida: string[] = []
  const src = join(raiz, 'src')
  try { if (lstatSync(src).isSymbolicLink()) return saida } catch { return saida }
  const andar = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.isSymbolicLink()) continue
      const p = join(dir, e.name)
      if (e.isDirectory()) {
        if (e.name !== 'node_modules') andar(p)
        continue
      }
      const rel = relDe(raiz, p)
      // Bundle ao lado do fonte (demo.js / demo.tsx) não é lido de novo: o .tsx já entra.
      const bundle = EXT_JS.test(e.name) && ['.ts', '.tsx', '.mts', '.cts'].some((ext) => {
        try { return lstatSync(join(raiz, rel.replace(/\.(?:js|jsx|mjs|cjs)$/, ext))).isFile() } catch { return false }
      })
      if ((EXT_TEXTO.test(e.name) || (herdados.has(rel) && !bundle)) && !foraDoGate(rel, modo, declarados)) saida.push(rel)
    }
  }
  andar(src)
  return saida.sort()
}

export function inventariar(raiz: string, modo: Modo = 'candidato'): Inventario {
  const inv: Inventario = {}
  for (const arq of arquivosDoApp(raiz, modo)) {
    for (const a of textosVisiveis(arq, readFileSync(join(raiz, arq), 'utf8'))) {
      const m = (inv[arq] ??= {})
      m[a.texto] = (m[a.texto] ?? 0) + 1
    }
  }
  return inv
}

export interface Resultado {
  /** Texto solto que o inventário não cobre: vai para um catálogo. */
  novos: Achado[]
  /** Entradas do inventário sem texto correspondente: apague (o inventário só encolhe). */
  sobras: { arquivo: string; texto: string; vezes: number }[]
}

export function comparar(atual: Inventario, legado: Inventario, achados: Achado[] = []): Resultado {
  const novos: Achado[] = []
  const sobras: Resultado['sobras'] = []
  const porArquivo = new Map<string, Achado[]>()
  for (const a of achados) porArquivo.set(a.arquivo, [...(porArquivo.get(a.arquivo) ?? []), a])
  for (const [arq, textos] of Object.entries(atual)) {
    for (const [texto, vezes] of Object.entries(textos)) {
      const permitidas = legado[arq]?.[texto] ?? 0
      if (vezes > permitidas) {
        const exemplos = (porArquivo.get(arq) ?? []).filter((a) => a.texto === texto).slice(permitidas)
        novos.push(...(exemplos.length ? exemplos : [{ arquivo: arq, linha: 0, texto }]))
      }
    }
  }
  for (const [arq, textos] of Object.entries(legado)) {
    for (const [texto, vezes] of Object.entries(textos)) {
      const agora = atual[arq]?.[texto] ?? 0
      if (agora < vezes) sobras.push({ arquivo: arq, texto, vezes: vezes - agora })
    }
  }
  return { novos, sobras }
}

export interface CategoriasPlural { obrigatorias: readonly string[]; permitidas: readonly string[] }

/**
 * Erros de paridade entre catálogos. Chaves comuns: as mesmas em todos os idiomas, sem texto vazio, com os
 * mesmos `{parâmetros}`. Plurais (a base `x` de `x.other` no pt-BR): cada idioma tem exatamente as categorias
 * obrigatórias do seu locale (Intl.PluralRules) e nenhuma fora das permitidas; `{n}` é opcional em cada forma
 * (há idiomas que o omitem), os outros parâmetros são os do `other` pt-BR.
 */
export function conferirCatalogos(
  catalogos: Record<string, Readonly<Record<string, string | undefined>>>,
  categorias: (idioma: string) => CategoriasPlural,
  fonte = 'pt-BR',
): string[] {
  const base = catalogos[fonte]
  if (!base) return [`catálogo fonte ${fonte} ausente`]
  const params = (s: string, semN = false) => [...new Set([...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]!))].filter((p) => !semN || p !== 'n').sort().join(',')
  const plurais = Object.keys(base).filter((k) => k.endsWith('.other')).map((k) => k.slice(0, -'.other'.length))
  const todas = ['zero', 'one', 'two', 'few', 'many', 'other']
  const varianteDe = (chave: string) => plurais.find((b) => chave.startsWith(b + '.') && todas.includes(chave.slice(b.length + 1)))
  const erros: string[] = []
  for (const [idioma, cat] of Object.entries(catalogos)) {
    for (const chave of Object.keys(base)) {
      if (varianteDe(chave)) continue
      const valor = cat[chave]
      if (typeof valor !== 'string') { erros.push(`${idioma}: falta a chave ${chave}`); continue }
      if (!valor.trim()) erros.push(`${idioma}: ${chave} está vazio`)
      if (params(valor) !== params(base[chave]!)) erros.push(`${idioma}: ${chave} tem parâmetros {${params(valor)}}, pt-BR tem {${params(base[chave]!)}}`)
    }
    const { obrigatorias, permitidas } = categorias(idioma)
    for (const b of plurais) {
      const esperado = params(base[`${b}.other`]!, true)
      for (const c of obrigatorias) if (typeof cat[`${b}.${c}`] !== 'string') erros.push(`${idioma}: plural ${b} sem a forma ${c}`)
      for (const c of todas) {
        const valor = cat[`${b}.${c}`]
        if (typeof valor !== 'string') continue
        if (!permitidas.includes(c)) erros.push(`${idioma}: plural ${b} tem a forma ${c}, que o idioma não usa`)
        if (!valor.trim()) erros.push(`${idioma}: ${b}.${c} está vazio`)
        if (params(valor, true) !== esperado) erros.push(`${idioma}: ${b}.${c} tem parâmetros {${params(valor, true)}}, pt-BR tem {${esperado}} (fora {n})`)
      }
    }
    for (const chave of Object.keys(cat)) if (!(chave in base) && !varianteDe(chave)) erros.push(`${idioma}: chave ${chave} não existe no pt-BR`)
  }
  return erros
}

/** O nome exportado por cada catálogo plano e índice de idioma (os imports antigos continuam valendo). */
const EXPORTADO: Record<string, string> = { 'pt-BR': 'ptBR', en: 'en', es: 'es', fr: 'fr', 'zh-CN': 'zhCN', 'ar-AE': 'arAE' }

/**
 * A forma dos catálogos por área, lida do código-fonte (antes de aceitar o agregado):
 * - nas pastas de idioma, só o índice e os módulos do registro, todos presentes; arquivo a mais é erro;
 * - módulo: só `import type` e `export const textos = { 'chave': 'texto', … }`; cada chave no prefixo do módulo
 *   (o mais longo do registro) e em uma parte só;
 * - índice: importa `textos` de cada módulo uma vez e exporta o agregado só com `...parte` de cada um;
 * - catálogo plano antigo: só `export { nome } from './<idioma>/index.ts'`; texto acrescentado ali reprova;
 * - o agregado em uso (`catalogos`, os CATALOGOS do motor) é exatamente a união das partes.
 */
export function conferirModulos(raiz: string, catalogos?: Record<string, Readonly<Record<string, string | undefined>>>): string[] {
  const erros: string[] = [...falhasEstruturais(raiz)]
  const ler = (rel: string) => {
    try { return ts.createSourceFile(rel, readFileSync(join(raiz, rel), 'utf8'), ts.ScriptTarget.Latest, true) } catch { erros.push(`${rel}: arquivo ausente`); return null }
  }
  const tipoSo = (n: ts.Statement) => ts.isImportDeclaration(n) && !!n.importClause?.isTypeOnly
  const especificador = (n: ts.ImportDeclaration | ts.ExportDeclaration) => n.moduleSpecifier && ts.isStringLiteral(n.moduleSpecifier) ? n.moduleSpecifier.text : null
  const exportConst = (n: ts.Statement, nome: string) => ts.isVariableStatement(n) && n.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword)
    && n.declarationList.flags & ts.NodeFlags.Const && n.declarationList.declarations.length === 1
    && ts.isIdentifier(n.declarationList.declarations[0]!.name) && n.declarationList.declarations[0]!.name.text === nome
    ? n.declarationList.declarations[0]! : null
  const semCast = (e: ts.Expression | undefined) => {
    while (e && (ts.isAsExpression(e) || ts.isSatisfiesExpression(e) || ts.isParenthesizedExpression(e))) e = e.expression
    return e
  }
  for (const idioma of IDIOMAS_CATALOGO) {
    const pasta = `${I18N}/${idioma}`
    // Arquivos da pasta: só os declarados.
    const andar = (rel: string): string[] => {
      try {
        return readdirSync(join(raiz, rel), { withFileTypes: true }).flatMap((e) => e.isDirectory() ? andar(`${rel}/${e.name}`) : [`${rel}/${e.name}`])
      } catch { return [] }
    }
    for (const arq of andar(pasta)) if (!DECLARADOS.has(arq)) erros.push(`${arq}: arquivo não registrado em registro.ts (todo texto de tradução fica num módulo registrado)`)
    // Módulos.
    const uniao: Record<string, string> = {}
    const origem: Record<string, string> = {}
    for (const { modulo } of MODULOS) {
      const rel = `${pasta}/${modulo}.ts`
      const sf = ler(rel)
      if (!sf) continue
      const decl = sf.statements.filter((n) => !tipoSo(n))
      const d = decl.length === 1 ? exportConst(decl[0]!, 'textos') : null
      const obj = semCast(d?.initializer)
      if (!obj || !ts.isObjectLiteralExpression(obj)) { erros.push(`${rel}: o módulo só pode ter import type e export const textos = { … }`); continue }
      for (const p of obj.properties) {
        const valor = ts.isPropertyAssignment(p) ? p.initializer : undefined
        if (!ts.isPropertyAssignment(p) || !ts.isStringLiteral(p.name) || !valor || !(ts.isStringLiteral(valor) || ts.isNoSubstitutionTemplateLiteral(valor))) {
          erros.push(`${rel}:${sf.getLineAndCharacterOfPosition(p.getStart(sf)).line + 1}: só 'chave': 'texto' literais`); continue
        }
        const chave = p.name.text
        const dono = moduloDaChave(chave)
        if (dono !== modulo) erros.push(`${rel}: ${chave} pertence a ${dono ?? 'nenhum módulo registrado'}`)
        if (chave in origem) erros.push(`${idioma}: ${chave} duplicada em ${origem[chave]} e ${modulo}`)
        else { origem[chave] = modulo; uniao[chave] = valor.text }
      }
    }
    // Índice.
    const relIndice = `${pasta}/index.ts`
    const indice = ler(relIndice)
    if (indice) {
      const importados = new Map<string, string>()
      let agregado: ts.ObjectLiteralExpression | null = null
      for (const n of indice.statements) {
        if (tipoSo(n)) continue
        const d = exportConst(n, EXPORTADO[idioma]!)
        const ini = semCast(d?.initializer)
        if (ts.isImportDeclaration(n)) {
          const b = n.importClause?.namedBindings
          const el = b && ts.isNamedImports(b) && !n.importClause?.name && b.elements.length === 1 ? b.elements[0]! : null
          const m = /^\.\/(.+)\.ts$/.exec(especificador(n) ?? '')?.[1]
          if (el && (el.propertyName ?? el.name).text === 'textos' && m && MODULOS.some((x) => x.modulo === m) && ![...importados.values()].includes(m)) importados.set(el.name.text, m)
          else erros.push(`${relIndice}: import fora do registro: ${n.getText(indice)}`)
        } else if (!agregado && ini && ts.isObjectLiteralExpression(ini)) agregado = ini
        else erros.push(`${relIndice}: o índice só importa os módulos e exporta ${EXPORTADO[idioma]} = { ...partes }`)
      }
      const espalhados = new Set<string>()
      for (const p of agregado?.properties ?? []) {
        const nome = ts.isSpreadAssignment(p) && ts.isIdentifier(p.expression) ? p.expression.text : null
        if (!nome || !importados.has(nome) || espalhados.has(nome)) erros.push(`${relIndice}: o agregado só leva ...parte, uma vez cada: ${p.getText(indice)}`)
        else espalhados.add(nome)
      }
      for (const { modulo } of MODULOS) {
        if (![...importados].some(([nome, m]) => m === modulo && espalhados.has(nome))) erros.push(`${relIndice}: módulo ${modulo} não entra no agregado`)
      }
    }
    // Catálogo plano: só reexporta.
    const relPlano = `${I18N}/${idioma}.ts`
    const plano = ler(relPlano)
    if (plano) {
      const ok = plano.statements.length > 0 && plano.statements.every((n) => ts.isExportDeclaration(n) && !n.isTypeOnly && especificador(n) === `./${idioma}/index.ts`
        && !!n.exportClause && ts.isNamedExports(n.exportClause) && n.exportClause.elements.every((e) => !e.propertyName && e.name.text === EXPORTADO[idioma]))
      if (!ok) erros.push(`${relPlano}: o caminho antigo só reexporta (export { ${EXPORTADO[idioma]} } from './${idioma}/index.ts'); texto novo vai no módulo da área`)
    }
    // O agregado em uso é a união das partes.
    const cat = catalogos?.[idioma]
    if (cat) {
      for (const [k, v] of Object.entries(cat)) if (uniao[k] !== v) erros.push(`${idioma}: o agregado tem ${k} diferente das partes`)
      for (const k of Object.keys(uniao)) if (!(k in cat)) erros.push(`${idioma}: ${k} está numa parte e falta no agregado`)
    }
  }
  return erros
}

const INVENTARIO = 'tools/textos-legado.json'
const INVENTARIO_NO_GIT = 'apps/desktop/tools/textos-legado.json'
const JS_LEGADO = 'tools/javascript-legado.json'
const JS_LEGADO_NO_GIT = 'apps/desktop/tools/javascript-legado.json'

type ListaJs = { arquivos: Set<string>; erro: string | null }

/** A lista de javascript-legado.json: `arquivos` é um array de textos. Formato errado vira `erro`, nunca uma lista vazia calada. */
function listaDeJavascript(texto: string): ListaJs {
  const invalido = (motivo: string): ListaJs => ({ arquivos: new Set(), erro: `javascript-legado.json inválido: ${motivo}` })
  let json: unknown
  try { json = JSON.parse(texto) } catch { return invalido('JSON quebrado') }
  const arquivos = typeof json === 'object' && json !== null && !Array.isArray(json) ? (json as { arquivos?: unknown }).arquivos : undefined
  if (!Array.isArray(arquivos)) return invalido('falta `arquivos`, um array de caminhos')
  if (!arquivos.every((a) => typeof a === 'string')) return invalido('`arquivos` tem item que não é texto')
  return { arquivos: new Set(arquivos as string[]), erro: null }
}

/**
 * O JavaScript em `src` que a main já tinha (tools/javascript-legado.json), lido da árvore: vale sem Git.
 * O arquivo é obrigatório (como textos-legado.json): ausente ou fora do formato dá `erro`.
 */
export function lerJavascriptLegado(raiz: string): ListaJs {
  let texto: string
  try { texto = readFileSync(join(raiz, JS_LEGADO), 'utf8') } catch { return { arquivos: new Set(), erro: 'javascript-legado.json inválido: arquivo ausente' } }
  return listaDeJavascript(texto)
}
const REGISTRO_NO_GIT = `apps/desktop/${I18N}/registro.ts`

export function lerInventario(raiz: string): Inventario {
  return (JSON.parse(readFileSync(join(raiz, INVENTARIO), 'utf8')) as { arquivos: Inventario }).arquivos
}

/** Confere a árvore inteira contra o inventário. `declarados` e `idiomas` só mudam na junção, já somados à main. */
export function conferirArvore(
  raiz: string,
  declarados: ReadonlySet<string> = DECLARADOS,
  idiomas: ReadonlySet<string> = new Set(IDIOMAS_CATALOGO),
): Resultado & { total: number; erros: string[] } {
  const erros = falhasEstruturais(raiz, idiomas)
  const achados = arquivosDoApp(raiz, 'candidato', declarados).flatMap((arq) => textosVisiveis(arq, readFileSync(join(raiz, arq), 'utf8')))
  const atual: Inventario = {}
  for (const a of achados) { const m = (atual[a.arquivo] ??= {}); m[a.texto] = (m[a.texto] ?? 0) + 1 }
  const legado = lerInventario(raiz)
  const total = Object.values(legado).reduce((s, m) => s + Object.values(m).reduce((x, y) => x + y, 0), 0)
  return { ...comparar(atual, legado, achados), total, erros }
}

export function gravarInventario(raiz: string, inv: Inventario) {
  const ordenado: Inventario = {}
  for (const arq of Object.keys(inv).sort()) {
    const textos = Object.keys(inv[arq]!).sort()
    if (textos.length) ordenado[arq] = Object.fromEntries(textos.map((t) => [t, inv[arq]![t]!]))
  }
  const cabecalho = 'DON-903: textos visíveis legados ainda fora dos catálogos (tools/textos-gate.ts). Só encolhe: migre o texto e apague a entrada.'
  writeFileSync(join(raiz, INVENTARIO), JSON.stringify({ sobre: cabecalho, arquivos: ordenado }, null, 1) + '\n')
}

/** `--encolher`: tira do inventário o que já saiu do código; nunca acrescenta. */
export function encolher(raiz: string): number {
  const legado = lerInventario(raiz)
  const atual = inventariar(raiz)
  let removidas = 0
  const novo: Inventario = {}
  for (const [arq, textos] of Object.entries(legado)) {
    for (const [texto, vezes] of Object.entries(textos)) {
      const fica = Math.min(vezes, atual[arq]?.[texto] ?? 0)
      removidas += vezes - fica
      if (fica) (novo[arq] ??= {})[texto] = fica
    }
  }
  gravarInventario(raiz, novo)
  return removidas
}

/** Git deve escolher o repositório pelo cwd, mesmo dentro de um hook. */
export function envGitSanitizado(env: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  return Object.fromEntries(Object.entries(env).filter(([chave]) => !chave.startsWith('GIT_')))
}

function gitBruto(cwd: string, args: string[], aceitar: readonly number[] = [0]): { status: number; stdout: string } {
  try {
    return { status: 0, stdout: execFileSync('git', args, { cwd, env: envGitSanitizado(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 1 << 30 }) }
  } catch (e) {
    const err = e as { status?: number; stdout?: string | Buffer }
    const status = typeof err.status === 'number' ? err.status : 1
    const stdout = typeof err.stdout === 'string' ? err.stdout : Buffer.isBuffer(err.stdout) ? err.stdout.toString('utf8') : ''
    if (aceitar.includes(status)) return { status, stdout }
    throw e
  }
}

const git = (cwd: string, args: string[]) => gitBruto(cwd, args).stdout.trim()

/**
 * Onde o Git enxerga este repositório a partir de `raiz`.
 * Work tree: a raiz do checkout. Snapshot do hook: a pasta fica dentro de `.git` e não é work tree
 * (`--show-toplevel` sai 128); `--git-dir` acha o repositório e a própria pasta serve de cwd.
 * Sem repositório, null: o JavaScript em `src` continua falhando.
 */
function pastaDoGit(raiz: string): string | null {
  try { return git(raiz, ['rev-parse', '--show-toplevel']) } catch { /* snapshot do hook, ou sem work tree */ }
  try {
    git(raiz, ['rev-parse', '--git-dir'])
    return raiz
  } catch {
    return null
  }
}

/** JavaScript em `apps/desktop/src` num commit. Caminhos relativos a `apps/desktop`. */
function javascriptNoCommit(cwd: string, commit: string): Set<string> {
  let lista = ''
  try { lista = git(cwd, ['ls-tree', '-r', '--name-only', commit, '--', 'apps/desktop/src']) } catch { return new Set() }
  const saida = new Set<string>()
  for (const caminho of lista.split('\n')) {
    if (caminho.startsWith('apps/desktop/') && EXT_JS.test(caminho)) saida.add(caminho.slice('apps/desktop/'.length))
  }
  return saida
}

/** A lista de JavaScript legado de um commit; sem o arquivo (antes desta regra), o JavaScript que o commit tinha em `src`. */
function javascriptLegadoNoCommit(cwd: string, commit: string): ListaJs {
  let texto: string
  try { texto = git(cwd, ['show', `${commit}:${JS_LEGADO_NO_GIT}`]) } catch { return { arquivos: javascriptNoCommit(cwd, commit), erro: null } }
  return listaDeJavascript(texto)
}

/** A lista da main, pelo commit já revisado. Não lê o arquivo do PR. */
function registroDaMain(cwd: string, ponta: string): { idiomas: string[]; modulos: string[] } | null {
  try { return registroExportado(git(cwd, ['show', `${ponta}:${REGISTRO_NO_GIT}`])) } catch { return null }
}

/** O que o PR exporta de verdade, mais idiomas e módulos que só a main já tem. */
function listaCombinada(main: { idiomas: string[]; modulos: string[] } | null): { idiomas: Set<string>; declarados: Set<string> } {
  const idiomas = new Set<string>(IDIOMAS_CATALOGO)
  const modulos = new Set(MODULOS.map((m) => m.modulo))
  if (main) {
    for (const idioma of main.idiomas) idiomas.add(idioma)
    for (const modulo of main.modulos) modulos.add(modulo)
  }
  const declarados = new Set<string>()
  for (const idioma of idiomas) {
    declarados.add(`${I18N}/${idioma}/index.ts`)
    for (const modulo of modulos) declarados.add(`${I18N}/${idioma}/${modulo}.ts`)
  }
  return { idiomas, declarados }
}

/** O texto solto de um commit, pelo arquivo (modo `base`: o catálogo plano antigo não conta). */
function inventarioNoCommit(cwd: string, tree: string): Inventario {
  const pasta = mkdtempSync(join(tmpdir(), 'textos-base-'))
  try {
    const tar = execFileSync('git', ['archive', '--format=tar', tree, 'apps/desktop/src'], { cwd, env: envGitSanitizado(), maxBuffer: 1 << 30, stdio: ['ignore', 'pipe', 'ignore'] })
    execFileSync('tar', ['-x', '-C', pasta], { input: tar })
    return inventariar(join(pasta, 'apps/desktop'), 'base')
  } finally {
    rmSync(pasta, { recursive: true, force: true })
  }
}

/**
 * Clone raso do Actions: a ponta da base entra com `--depth=1` e o ancestral ainda não está no disco.
 * Aprofunda os dois lados até o `merge-base` existir. Histórico completo só se isso não bastar.
 */
function ancestral(cwd: string, ponta: string, env: NodeJS.ProcessEnv): string {
  const achar = () => git(cwd, ['merge-base', 'HEAD', ponta])
  try { return achar() } catch { /* histórico curto */ }
  if (git(cwd, ['rev-parse', '--is-shallow-repository']) !== 'true') throw new Error('sem merge-base')
  const baseRef = env.GITHUB_BASE_REF
  const headRef = env.GITHUB_HEAD_REF
  if (!baseRef) throw new Error('sem merge-base')
  for (const n of [64, 256, 1024, 4096]) {
    try { git(cwd, ['fetch', '--quiet', `--deepen=${n}`, 'origin', baseRef]) } catch { /* a base pode já estar completa */ }
    if (headRef) try { git(cwd, ['fetch', '--quiet', `--deepen=${n}`, 'origin', headRef]) } catch { /* branch fora desse remoto */ }
    try { return achar() } catch { /* continua */ }
  }
  try { git(cwd, ['fetch', '--quiet', '--unshallow', 'origin']) } catch { /* segue */ }
  return achar()
}

export type Base = { ref: string; ponta: string; cwd: string; inventario: Inventario } | { motivo: string }

/**
 * O texto solto real do código do merge-base, lido do Git e inventariado com as mesmas regras (o JSON pode ser
 * editado no mesmo patch; o código do merge-base, não). No `npm test`, só num PR do GitHub Actions: busca a
 * ponta de `GITHUB_BASE_REF` e usa `git merge-base` com HEAD. No CI, `MEXERICA_CI_BASE_SHA`
 * exige a base do evento já baixada, sem rede nem fallback. `local` (o CLI): merge-base com `origin/main`.
 * Sem esse contexto (hook, push na main, snapshot da Instabox sem .git): `{ motivo }`, e vale a conferência da árvore.
 * `ponta` é a ponta da main, para simular a junção sem usar a ponta como base da comparação.
 */
export function inventarioDaBase(raiz: string, env: NodeJS.ProcessEnv = process.env, local = false): Base {
  if (!env.GITHUB_BASE_REF && !local) return { motivo: 'fora de um PR do GitHub Actions' }
  // `cwd` onde o caminho `apps/desktop/src` vale: raiz do work tree, ou a própria pasta no snapshot do hook.
  const cwd = pastaDoGit(raiz)
  if (!cwd) return { motivo: 'sem repositório Git' }
  let ref: string
  let ponta: string
  try {
    if (env.GITHUB_BASE_REF && env.MEXERICA_CI_BASE_SHA !== undefined) {
      // O checkout autenticado com a base termina antes dos testes, sem persistir token.
      // O SHA vem do evento do workflow; não substitua uma base ausente por fetch/fallback.
      const baseSha = env.MEXERICA_CI_BASE_SHA
      if (!/^[a-f0-9]{40}$/.test(baseSha) || /^0+$/.test(baseSha)) throw new Error('SHA da base inválido')
      ponta = git(cwd, ['rev-parse', '--verify', `${baseSha}^{commit}`])
      if (ponta !== baseSha) throw new Error('base divergente')
      ref = git(cwd, ['merge-base', 'HEAD', ponta])
    } else if (env.GITHUB_BASE_REF) {
      // Raso só se o checkout já é raso (o do Actions): num clone completo, --depth o deixaria raso.
      const raso = git(cwd, ['rev-parse', '--is-shallow-repository']) === 'true'
      git(cwd, ['fetch', '--quiet', ...(raso ? ['--depth=1'] : []), 'origin', env.GITHUB_BASE_REF])
      ponta = git(cwd, ['rev-parse', 'FETCH_HEAD'])
      ref = ancestral(cwd, ponta, env)
    } else {
      ponta = git(cwd, ['rev-parse', 'origin/main'])
      ref = git(cwd, ['merge-base', 'HEAD', ponta])
    }
  } catch {
    return { motivo: 'base indisponível (sem origin/main ou GITHUB_BASE_REF)' }
  }
  return { ref, ponta, cwd, inventario: inventarioNoCommit(cwd, ref) }
}

/**
 * Junção com a ponta da main, sem checkout. Texto solto novo falha. Sobra do próprio PR falha.
 * Sobra que existia no merge-base e a main tirou do código só avisa. Conflito no inventário não reprova:
 * o merge já fica bloqueado e o PR traz a main.
 */
function simularJuncao(cwd: string, ponta: string, mb: Inventario, raiz: string): { erros: string[]; avisos: string[] } {
  const erros: string[] = []
  const avisos: string[] = []
  const saida = gitBruto(cwd, ['merge-tree', '--write-tree', '--no-messages', '--name-only', 'HEAD', ponta], [0, 1])
  const linhas = saida.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)
  const tree = linhas.find((l) => /^[0-9a-f]{40,64}$/i.test(l))
  if (!tree) throw new Error('merge-tree não devolveu a árvore')
  const conflitos = new Set<string>()
  for (const l of linhas) {
    if (l === tree || l.startsWith('Auto-merging')) continue
    const aviso = /^CONFLICT \([^)]+\): Merge conflict in (.+)$/.exec(l)
    if (aviso?.[1]) conflitos.add(aviso[1])
    else if (!l.startsWith('CONFLICT')) conflitos.add(l)
  }
  if (conflitos.has(INVENTARIO_NO_GIT)) {
    avisos.push('textos-legado.json conflita na junção com a main; o merge já fica bloqueado')
    return { erros, avisos }
  }
  const pasta = mkdtempSync(join(tmpdir(), 'textos-juncao-'))
  try {
    const lista = listaCombinada(registroDaMain(cwd, ponta))
    erros.push(...falhasEstruturais(raiz, lista.idiomas))
    const tar = execFileSync('git', ['archive', '--format=tar', tree, 'apps/desktop'], { cwd, env: envGitSanitizado(), maxBuffer: 1 << 30, stdio: ['ignore', 'pipe', 'ignore'] })
    execFileSync('tar', ['-x', '-C', pasta], { input: tar })
    const merged = conferirArvore(join(pasta, 'apps/desktop'), lista.declarados, lista.idiomas)
    erros.push(...merged.erros.map((e) => `junção com a main: ${e}`))
    for (const a of merged.novos) erros.push(`junção com a main tem texto solto: ${a.arquivo}:${a.linha} ${JSON.stringify(a.texto)}`)
    if (merged.sobras.length) {
      const doPr = new Set(conferirArvore(raiz).sobras.map((s) => `${s.arquivo}\0${s.texto}`))
      const mainInv = inventarioNoCommit(cwd, ponta)
      for (const s of merged.sobras) {
        if (doPr.has(`${s.arquivo}\0${s.texto}`)) erros.push(`sobra do próprio PR: ${s.arquivo} ${JSON.stringify(s.texto)} ×${s.vezes}`)
        else if ((mb[s.arquivo]?.[s.texto] ?? 0) > (mainInv[s.arquivo]?.[s.texto] ?? 0)) avisos.push(`sobra vinda da main: ${s.arquivo} ${JSON.stringify(s.texto)} ×${s.vezes}`)
        else erros.push(`sobra na junção: ${s.arquivo} ${JSON.stringify(s.texto)} ×${s.vezes}`)
      }
    }
  } finally {
    rmSync(pasta, { recursive: true, force: true })
  }
  return { erros, avisos }
}

/**
 * A regra de não crescer, para o teste e o CLI. Num PR (ou no CLI, `local`), compara o inventário com o
 * merge-base e simula a junção com a ponta da main. Fora dele, sem base: só `aviso`, e vale a conferência da árvore.
 * Base indisponível num PR reprova.
 */
export function naoCresceu(raiz: string, env: NodeJS.ProcessEnv = process.env, local = false): { erros: string[]; aviso?: string; avisos: string[] } {
  const base = inventarioDaBase(raiz, env, local)
  if ('motivo' in base) {
    return env.GITHUB_BASE_REF ? { erros: [`base indisponível num PR: ${base.motivo}`], avisos: [] } : { erros: [], aviso: base.motivo, avisos: [] }
  }
  const cresceu = comparar(lerInventario(raiz), base.inventario).novos
    .map((a) => `inventário cresceu em relação à base ${base.ref}: ${a.arquivo} ${JSON.stringify(a.texto)}`)
  const naBase = javascriptLegadoNoCommit(base.cwd, base.ref)
  const cresceuJs = [
    ...(naBase.erro ? [`base ${base.ref}: ${naBase.erro}`] : []),
    ...[...lerJavascriptLegado(raiz).arquivos].filter((a) => !naBase.arquivos.has(a))
      .map((a) => `javascript-legado.json cresceu em relação à base ${base.ref}: ${a}`),
  ]
  const juncao = simularJuncao(base.cwd, base.ponta, base.inventario, raiz)
  return { erros: [...cresceu, ...cresceuJs, ...juncao.erros], avisos: juncao.avisos }
}

/** Áreas para `--listar` (tradutores e PRs de troca), pelo caminho; a primeira que casa vale. */
export const AREAS: readonly { id: string; nome: string; caminho: RegExp }[] = [
  { id: 'idiomas', nome: 'Idiomas (base DON-903)', caminho: /^src\/renderer\/(views\/fork\/idioma|lib\/fork-i18n)\// },
  { id: 'entrada', nome: 'Entrada e conta', caminho: /^src\/(renderer\/views\/(welcome\/|Account|fork\/account\/)|main\/(fork-account\/|account|auth)|shared\/fork-account)/ },
  { id: 'ajustes', nome: 'Ajustes', caminho: /^src\/renderer\/views\/(settings|fork\/(ajustes|contas-ia|bot-de-fundo|voz))\// },
  { id: 'sessoes', nome: 'Sessões', caminho: /^src\/renderer\/views\/fork\/(sessoes|abas|painel-direito)\// },
  { id: 'conversa', nome: 'Conversa', caminho: /^src\/renderer\/views\/(thread|fork\/(mensagens|canal|genui|pergunta-dono))\// },
  { id: 'criar-bot', nome: 'Criar bot', caminho: /^src\/renderer\/views\/(criar-bot|fork\/(bot-settings|bot-panel))\// },
  { id: 'equipes', nome: 'Bots e Equipes', caminho: /^src\/renderer\/views\/(bots|bot-groups|fork\/(times|times-sidebar|torre|donna))\/|^src\/renderer\/views\/Bot/ },
  { id: 'rotinas', nome: 'Rotinas', caminho: /^src\/renderer\/views\/(routines|rotinas-central)\// },
  { id: 'whatsapp', nome: 'WhatsApp', caminho: /^src\/renderer\/views\/fork\/whatsapp\// },
  { id: 'extensoes', nome: 'Extensões, Skills e Conectores', caminho: /^src\/renderer\/views\/(marketplace|fork\/(extensoes|catalogo-skills|meus-mcp|conector-card|skill-drafts|biblioteca))\// },
  { id: 'spaces', nome: 'Space e Pessoal', caminho: /^src\/renderer\/views\/fork\/spaces\// },
  { id: 'main', nome: 'Processo principal (menus, bandeja, diálogos)', caminho: /^src\/(main|preload)\// },
  { id: 'outros', nome: 'Demais telas e compartilhados', caminho: /./ },
]

export function areaDe(arquivo: string): string {
  return AREAS.find((a) => a.caminho.test(arquivo))!.id
}

/** `--listar`: os textos soltos que ainda faltam migrar, por área, com arquivo:linha. Só lê. */
export function listar(raiz: string, area?: string): (Achado & { area: string })[] {
  return arquivosDoApp(raiz)
    .filter((arq) => !area || areaDe(arq) === area)
    .flatMap((arq) => textosVisiveis(arq, readFileSync(join(raiz, arq), 'utf8')).map((a) => ({ ...a, area: areaDe(arq) })))
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const raiz = new URL('..', import.meta.url).pathname
  const args = process.argv.slice(2)
  if (args.includes('--listar')) {
    const i = args.indexOf('--area')
    const area = i >= 0 ? args[i + 1] : undefined
    if (area && !AREAS.some((a) => a.id === area)) {
      console.error(`área desconhecida: ${area}. Áreas: ${AREAS.map((a) => a.id).join(', ')}`)
      process.exit(2)
    }
    const itens = listar(raiz, area)
    if (args.includes('--json')) console.log(JSON.stringify(itens, null, 1))
    else {
      for (const a of AREAS) {
        const daArea = itens.filter((x) => x.area === a.id)
        if (!daArea.length) continue
        console.log(`## ${a.nome} (${a.id}): ${daArea.length}`)
        for (const x of daArea) console.log(`${x.arquivo}:${x.linha}\t${x.texto}`)
      }
    }
  } else if (args.includes('--encolher')) {
    console.log(`Inventário: ${encolher(raiz)} entrada(s) removida(s).`)
  } else {
    const r = conferirArvore(raiz)
    for (const e of r.erros) console.error(e)
    for (const a of r.novos) console.error(`texto solto: ${a.arquivo}:${a.linha} ${JSON.stringify(a.texto)}`)
    for (const s of r.sobras) console.error(`sobra no inventário: ${s.arquivo} ${JSON.stringify(s.texto)} ×${s.vezes}`)
    const base = naoCresceu(raiz, process.env, true)
    for (const e of base.erros) console.error(e)
    for (const a of base.avisos) console.error(`aviso: ${a}`)
    console.log(`Legado inventariado: ${r.total} texto(s).${base.aviso ? ` Sem comparação com a base: ${base.aviso}.` : ''}`)
    process.exitCode = r.novos.length || r.sobras.length || r.erros.length || base.erros.length ? 1 : 0
  }
}
