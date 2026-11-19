import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'
import { createElement, isValidElement, type ReactNode } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'

// D440 (P49=B, substitui a D407), D70: as três funções do Motor aparecem apagadas, com selo, e nada nelas
// age. O componente e os textos são os reais; só o CardSection é trocado por um que guarda a árvore de linhas.
const sourceRoot = new URL('../../../../', import.meta.url).href
const stubs = new Map([
  [new URL('../../../components/Controls.tsx', import.meta.url).href,
    "import { createElement } from 'react'\nexport function CardSection(props) { globalThis.__secao = props; return createElement('section', null, createElement('h3', null, props.title), props.children, createElement('footer', null, props.footer)) }"],
])
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      for (const extension of ['.ts', '.tsx', '/index.ts']) {
        const candidate = new URL(`${specifier}${extension}`, context.parentURL)
        if (existsSync(candidate)) return next(candidate.href, context)
      }
    }
    return next(specifier, context)
  },
  load(url, context, next) {
    if (!url.startsWith('file:')) return next(url, context)
    const clean = new URL(url)
    clean.search = ''
    const stub = stubs.get(clean.href)
    if (stub !== undefined) return { format: 'module', shortCircuit: true, source: stub }
    if (/\.css$/.test(clean.pathname)) return { format: 'module', shortCircuit: true, source: 'export default ""' }
    if (clean.href.startsWith(sourceRoot) && /\.tsx?$/.test(clean.pathname)) {
      const source = ts.transpileModule(readFileSync(clean, 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
      }).outputText
      return { format: 'module', shortCircuit: true, source }
    }
    return next(url, context)
  },
})
Object.assign(globalThis, {
  window: { addEventListener() {} },
  document: { documentElement: {}, body: { setAttribute() {} }, hidden: false, hasFocus: () => true },
  // Os textos conferidos são os do pt-BR: idioma escolhido pt-BR (sem escolha, o padrão é o inglês, D454/D495).
  localStorage: { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} },
})

const { MotorIndisponiveis } = await import('./MotorIndisponiveis.tsx')
const { StoreContext } = await import('../../../store/context.ts')

/** Props de todos os elementos da árvore entregue ao cartão. */
function props(node: ReactNode, out: Record<string, unknown>[] = []): Record<string, unknown>[] {
  if (Array.isArray(node)) node.forEach((n) => props(n, out))
  else if (isValidElement(node)) { const p = node.props as Record<string, unknown>; out.push(p); props(p.children as ReactNode, out) }
  return out
}

test('Motor mostra auto, alternativas e guias apagados, com selo, sem handlers nem leitura do store', () => {
  const chamadas: string[] = []
  // Qualquer leitura do store (updateBot, request, roteamento…) fica registrada.
  const store = new Proxy({}, { get: (_, key) => { chamadas.push(String(key)); return () => { chamadas.push(`chamou ${String(key)}`) } } })

  const html = renderToStaticMarkup(createElement(StoreContext.Provider, { value: store as never }, createElement(MotorIndisponiveis)))

  const linhas = [...html.matchAll(/<div[^>]*data-funcao="([a-z]+)"[^>]*>/g)]
  assert.deepEqual(linhas.map((l) => l[1]), ['auto', 'reservas', 'guias'])
  for (const [tag] of linhas) {
    assert.match(tag, /aria-disabled="true"/)
    assert.match(tag, /role="group"/)
  }
  for (const nome of ['Escolha automática', 'Contas alternativas (reservas)', 'Guias de escolha']) {
    assert.ok(html.includes(`>${nome}<span class="bot-settings-futura-selo">ainda indisponível</span>`), nome)
  }
  assert.equal(html.match(/ainda indisponível/g)?.length, 3)
  assert.ok(html.includes('Por enquanto, as conversas novas usam a conta e o modelo escolhidos acima.'))
  // R1-01: o rodapé não repete o aviso das conversas abertas, que fica no cartão do modelo.
  assert.doesNotMatch(html, /conversa aberta/)
  // Nada focável nem acionável: sem botão, campo, link ou tabindex; nenhum on* em elemento algum.
  assert.doesNotMatch(html, /<(button|input|select|textarea|a)\b|tabindex/i)
  const arvore = props((globalThis as { __secao?: { children: ReactNode } }).__secao?.children)
  assert.equal(arvore.filter((p) => p['aria-disabled'] === 'true').length, 3)
  assert.deepEqual(arvore.flatMap((p) => Object.keys(p).filter((k) => /^on[A-Z]/.test(k) || k === 'tabIndex')), [])
  // Sem handler, clicar ou teclar não tem a quem chamar; o store não é lido durante o render.
  // Limite (R1-03): SSR não executa efeitos de montagem; o componente não usa efeitos nem store.
  assert.deepEqual(chamadas, [])
})
