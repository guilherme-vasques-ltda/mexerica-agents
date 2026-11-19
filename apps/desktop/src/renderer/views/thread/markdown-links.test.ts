import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createContext, Script } from 'node:vm'
import test from 'node:test'
import { Children, isValidElement, memo, type MouseEvent, type ReactNode } from 'react'
import * as jsxRuntime from 'react/jsx-runtime'
import ts from 'typescript'
import { decideNavigation } from '../../../shared/navigation-policy.ts'
import { parseMarkdown } from './markdown-blocks.ts'

function fixture() {
  const opened: string[] = []
  const url = new URL('./Markdown.tsx', import.meta.url)
  const output = ts.transpileModule(readFileSync(url, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
    fileName: url.pathname,
  }).outputText
  const dependencies: Record<string, unknown> = {
    react: { memo },
    'react/jsx-runtime': jsxRuntime,
    '../../../shared/navigation-policy': { decideNavigation },
    './markdown-blocks.ts': { parseMarkdown },
    '../../components/Controls': {},
    '../../components/Stack': {},
    '../../components/Text': {},
    './markdown.css': {},
    '../fork/mensagens/ritmo-markdown.css': {},
    // D165 fase F: tabelas viram DataTable; o teste de links não renderiza tabelas.
    './cards/DataTable': {},
  }
  const exports = {} as { inline: (text: string) => ReactNode[] }
  new Script(output, { filename: url.pathname }).runInContext(createContext({
    exports,
    require: (name: string) => {
      if (!(name in dependencies)) throw new Error(`Dependência não simulada: ${name}`)
      return dependencies[name]
    },
    window: { codync: { app: { openExternal: (target: string) => { opened.push(target) } } } },
  }))
  const link = (target: string) => {
    const nodes = exports.inline(`[Destino](${target})`)
    assert.equal(nodes.length, 1)
    const node = nodes[0]
    assert.ok(isValidElement<{ title: string; href?: string; className?: string; onClick?: (event: MouseEvent) => void; children: ReactNode }>(node))
    assert.equal(node.props.title, target)
    assert.deepEqual(Children.toArray(node.props.children), ['Destino'])
    return node
  }
  return { link, opened }
}

test('links web e e-mail mantêm tooltip e abrem somente pela ponte externa', () => {
  const { link, opened } = fixture()
  const urls = ['http://example.test/', 'HTTPS://example.test/path?q=value', 'mailto:person@example.test']
  for (const url of urls) {
    const node = link(url)
    assert.equal(node.type, 'a')
    assert.equal(node.props.href, url)
    assert.equal(node.props.className, 'md-link')
    let prevented = false
    node.props.onClick!({ preventDefault: () => { prevented = true } } as MouseEvent)
    assert.equal(prevented, true)
  }
  assert.deepEqual(opened, urls)
})

test('esquemas bloqueados aparecem como texto sem ação e preservam o endereço no tooltip', () => {
  const { link, opened } = fixture()
  for (const url of ['file:///tmp/private.txt', 'vscode://file/tmp/private.txt',
    'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
    'javascript:void', 'data:text/html,example', 'donnabot://oauth', '/relative', 'https://']) {
    const node = link(url)
    assert.equal(node.type, 'span')
    assert.equal(node.props.href, undefined)
    assert.equal(node.props.className, undefined)
    assert.equal(node.props.onClick, undefined)
  }
  assert.deepEqual(opened, [])
})
