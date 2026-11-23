import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, matchesGlob, relative, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import ts from 'typescript'

const desktop = fileURLToPath(new URL('../../../../../', import.meta.url))
const src = join(desktop, 'src')
const testeSSR = fileURLToPath(import.meta.url)
const componente = join(dirname(testeSSR), 'ConsentimentoPlanoChatGPT.tsx')
const protegidos = [componente, testeSSR]
const config = ts.readConfigFile(join(desktop, 'tsconfig.json'), ts.sys.readFile)
assert.equal(config.error, undefined)
const { options, errors } = ts.convertCompilerOptionsFromJson(config.config.compilerOptions, desktop)
assert.deepEqual(errors, [])
const aliases = Object.keys(options.paths ?? {}).map(nome => nome.split('*', 1)[0]!)
const canonico = (arquivo: string) => {
  const caminho = resolve(ts.sys.realpath?.(arquivo) ?? arquivo)
  return ts.sys.useCaseSensitiveFileNames ? caminho : caminho.toLowerCase()
}
const destinosProtegidos = new Set(protegidos.map(canonico))
const cache = ts.createModuleResolutionCache(desktop, canonico, options)

function referenciasProtegidas(arquivo: string, codigo: string): string[] {
  const fonte = ts.createSourceFile(arquivo, codigo, ts.ScriptTarget.Latest, true)
  const achados: string[] = []
  const conferir = (argumento: ts.Node | undefined) => {
    if (!argumento || !ts.isStringLiteralLike(argumento)) return
    const nome = argumento.text.split(/[?#]/, 1)[0]!
    if (!nome.startsWith('.') && !nome.startsWith('/') && !aliases.some(alias => nome.startsWith(alias))) return
    const modulo = nome.startsWith('/src/') ? join(desktop, nome) : nome
    const destino = ts.resolveModuleName(modulo, arquivo, options, ts.sys, cache).resolvedModule?.resolvedFileName
    if (destino && destinosProtegidos.has(canonico(destino))) achados.push(argumento.text)
  }
  const visitar = (no: ts.Node) => {
    if (ts.isImportDeclaration(no) || ts.isExportDeclaration(no)) conferir(no.moduleSpecifier)
    if (ts.isImportEqualsDeclaration(no) && ts.isExternalModuleReference(no.moduleReference)) conferir(no.moduleReference.expression)
    if (ts.isCallExpression(no)) {
      const chamada = no.expression
      if (chamada.kind === ts.SyntaxKind.ImportKeyword || (ts.isIdentifier(chamada) && chamada.text === 'require')) {
        conferir(no.arguments[0])
      }
      if (ts.isPropertyAccessExpression(chamada) && chamada.name.text === 'glob' && ts.isMetaProperty(chamada.expression)) {
        const arg = no.arguments[0]
        const padroes = arg && ts.isArrayLiteralExpression(arg) ? arg.elements : [arg]
        for (const padrao of padroes) {
          if (!padrao || !ts.isStringLiteralLike(padrao) || padrao.text.startsWith('!')) continue
          const nome = padrao.text
          const absoluto = nome.startsWith('/') ? join(desktop, nome)
            : nome.startsWith('@shared/') ? join(src, 'shared', nome.slice('@shared/'.length)) : resolve(dirname(arquivo), nome)
          if (protegidos.some(destino => matchesGlob(destino, absoluto))) achados.push(nome)
        }
      }
    }
    ts.forEachChild(no, visitar)
  }
  visitar(fonte)
  return achados
}

test('somente a fixture SSR pode importar o consentimento interno', () => {
  const vizinho = join(dirname(componente), 'produto-ficticio.tsx')
  for (const codigo of [
    `import './ConsentimentoPlanoChatGPT'`,
    `export { ConsentimentoPlanoChatGPT } from './ConsentimentoPlanoChatGPT.tsx'`,
    `void import(\`./ConsentimentoPlanoChatGPT?raw\`)`,
    `require('./ConsentimentoPlanoChatGPT')`,
    `import '/src/renderer/views/fork/contas-ia/ConsentimentoPlanoChatGPT'`,
    `import '@shared/../renderer/views/fork/contas-ia/ConsentimentoPlanoChatGPT'`,
    `import.meta.glob('./*.tsx')`,
    `import './consentimento-plano-chatgpt.test'`,
  ]) assert.equal(referenciasProtegidas(vizinho, codigo).length, 1, codigo)
  assert.deepEqual(referenciasProtegidas(vizinho, `// import './ConsentimentoPlanoChatGPT'`), [])

  const consumidores = ts.sys.readDirectory(src, ['.ts', '.tsx', '.mts', '.cts', '.js', '.jsx', '.mjs', '.cjs'])
    .filter(arquivo => arquivo !== testeSSR)
    .flatMap(arquivo => referenciasProtegidas(arquivo, readFileSync(arquivo, 'utf8'))
      .map(referencia => `${relative(src, arquivo)} → ${referencia}`))
  assert.deepEqual(consumidores, [])
})

const bundle = await build({
  stdin: {
    contents: `import { createElement } from 'react';
      import { renderToStaticMarkup } from 'react-dom/server';
      import { ConsentimentoPlanoChatGPT } from './ConsentimentoPlanoChatGPT';
      export const render = idioma => renderToStaticMarkup(createElement(ConsentimentoPlanoChatGPT, { idioma }));`,
    resolveDir: fileURLToPath(new URL('.', import.meta.url)),
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  packages: 'external',
})
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), Intl })
const { render } = module.exports as { render: (idioma: string) => string }

test('a tela interna mostra consentimento indisponível sem ação', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE']) {
    const html = render(idioma)
    const input = html.match(/<input[^>]+>/)?.[0] ?? ''
    assert.match(input, /role="switch"/)
    assert.match(input, /aria-label="[^"]+"/)
    assert.match(input, /aria-checked="false"/)
    assert.match(input, /\sdisabled(?:="")?/)
    assert.doesNotMatch(input, /\schecked(?:="")?/)
    assert.match(html, /https:\/\/chatgpt\.com\/settings\/usage/)
    assert.doesNotMatch(html, /<(?:a|button|form|select)\b|Testar agora|href=/)
  }
  const pt = render('pt-BR')
  assert.match(pt, /recurso ainda não está disponível/)
  assert.match(pt, /créditos extras/)
  assert.match(pt, /não verifica nem muda.*recarga automática/)
  assert.match(pt, /3 tentativas em 24 horas/)
  assert.match(pt, /não limita o valor cobrado/)
  assert.match(render('ar-AE'), /dir="rtl"/)
})
