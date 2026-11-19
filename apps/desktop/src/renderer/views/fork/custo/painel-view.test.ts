// MX-145 E2: o render real do painel (`PainelUso`, SSR no Node, como arquivar.test.ts): os limites do plano aparecem
// sempre e nunca dentro de uma conta; nomes e códigos entram isolados em árabe; "(parcial)" vem junto ao valor; rosto e
// selo nas linhas de conta e de bot.
import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import type { PainelProps } from './PainelUso.tsx'
import type { Totais } from './modelo.ts'

const bundle = await build({
  stdin: {
    contents: `export { PainelUso } from './views/fork/custo/PainelUso';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /(^\.\/|\/components\/)Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }, { name: 'rostos', setup(b) {
    // Os rostos (mexeriquinha, avatares) têm estado próprio e não fazem parte do que se confere: só a marca de que há um rosto.
    b.onResolve({ filter: /\/components\/Avatar$/ }, () => ({ path: 'rostos', namespace: 'rostos' }))
    b.onLoad({ filter: /.*/, namespace: 'rostos' }, () => ({ contents: `import { createElement } from 'react'
      export const BotAvatar = () => createElement('span', { 'data-rosto': 'bot' })
      export const CharacterAvatar = () => createElement('span', { 'data-rosto': 'personagem' })`, resolveDir: new URL('../../../../../', import.meta.url).pathname, loader: 'js' }))
  } }, { name: 'agent-icon', setup(b) {
    // `import.meta.glob` é do Vite; o logo do provedor não faz parte do que se confere aqui.
    b.onResolve({ filter: /\/components\/AgentIcon$/ }, () => ({ path: 'agent-icon', namespace: 'agent-icon' }))
    b.onLoad({ filter: /.*/, namespace: 'agent-icon' }, () => ({ contents: 'export function AgentIcon() { return null }; export const agentIconURL = () => null', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, location: { search: '' } }
type Modulo = {
  PainelUso: (p: PainelProps) => unknown
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
// O idioma escolhido é lido uma vez, ao carregar: um contexto novo por idioma (sem escolha salva o padrão é o inglês).
const carregar = (idioma: string): Modulo => {
  const storage = { getItem: (k: string) => (k === 'appLanguage' ? JSON.stringify(idioma) : null), setItem() {}, removeItem() {} }
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask, Intl, Date })
  return module.exports as Modulo
}
const modulos: Record<string, Modulo> = {}

const totais = (o: Partial<Totais> = {}): Totais => ({ turns: 4, unpricedTurns: 0, inputTokens: 1000, cachedReadTokens: null, cachedWriteTokens: null, outputTokens: 500, costs: [], ...o })
const usd = (amount: number) => [{ amount, currency: 'USD', source: 'motor' }]
const janela = { id: 'w', label: '5-hour', percent: 42 }
const usage = { providers: [{ id: 'claude', name: 'Claude', windows: [janela], source: 'x', updatedAt: 1 }] }
const limites = [{ chave: 'mac', rotulo: null, usage }]
const relatorio = (o: Record<string, unknown> = {}) => ({
  hubId: 'hub', measuredAt: 1, month: '2026-11', total: totais({ turns: 10, costs: usd(9), unpricedTurns: 2 }),
  bots: [{ botId: 'b1', name: 'Berga', totals: totais({ turns: 10, costs: usd(9), unpricedTurns: 2 }) }], conversations: [], helpers: [], conversation: null,
  contas: [{ accountId: 'acc-1', totals: totais({ costs: usd(2) }) }, { accountId: null, totals: totais({ turns: 6 }) }],
  modelos: [{ model: 'claude-opus-5-5', totals: totais() }, { model: null, totals: totais({ turns: 6 }) }], ...o,
})
const base: PainelProps = {
  estado: { kind: 'pronto', relatorio: relatorio() as never }, lendo: false, online: true, mes: '2026-11', onMes() {}, onAtualizar() {},
  aba: 'conta', cadastro: { kind: 'pronto', contas: [{ id: 'acc-1', label: 'Claude', backend: 'claude', billing: 'subscription' }] }, bot: () => undefined, limites,
}
const render = (o: Partial<PainelProps> = {}, idioma = 'pt-BR') => {
  const m = (modulos[idioma] ??= carregar(idioma))
  return m.renderToStaticMarkup(m.createElement(m.PainelUso as never, { ...base, ...o }))
}
const barras = (html: string) => (html.match(/role="progressbar"/g) ?? []).length

test('limites do plano: seção própria por provedor, em toda aba, e nunca dentro de uma conta', () => {
  for (const aba of ['conta', 'modelo', 'bot'] as const) {
    const html = render({ aba })
    assert.match(html, /Limites do plano/, aba)
    assert.equal(barras(html), 1, `${aba}: uma barra (a do provedor)`)
    assert.match(html, /Claude/)
  }
  // A linha da conta (cadastro com a assinatura do Claude) não carrega barra: a barra só vem depois do título da seção.
  const html = render({ aba: 'conta' })
  const secao = html.indexOf('Limites do plano')
  assert.ok(secao > 0)
  assert.equal(barras(html.slice(0, secao)), 0, 'nenhuma barra antes da seção de limites, ou seja, dentro de uma conta')
  assert.doesNotMatch(html.slice(0, secao), /Renova|renova/)
})

test('limites do plano não somem com mês vazio, falha do relatório, Hub antigo, 403 ou sem conexão', () => {
  const vazio = relatorio({ total: totais({ turns: 0, unpricedTurns: 0, inputTokens: null, outputTokens: null }), bots: [], contas: [], modelos: [] })
  const cenas: [string, Partial<PainelProps>][] = [
    ['mês vazio', { estado: { kind: 'pronto', relatorio: vazio as never } }],
    ['falha', { estado: { kind: 'falha', falha: 'falha' } }],
    ['relatório inválido', { estado: { kind: 'falha', falha: 'invalida' } }],
    ['Hub sem o RPC', { estado: { kind: 'falha', falha: 'antigo' } }],
    ['só o dono vê', { estado: { kind: 'falha', falha: 'soDono' } }],
    ['lendo', { estado: { kind: 'lendo' } }],
    ['sem conexão', { estado: { kind: 'lendo' }, online: false }],
    ['Hub antigo sem accounts/models', { estado: { kind: 'pronto', relatorio: relatorio({ contas: null, modelos: null }) as never } }],
  ]
  for (const [nome, o] of cenas) {
    for (const aba of ['conta', 'modelo', 'bot'] as const) {
      const html = render({ ...o, aba })
      assert.match(html, /Limites do plano/, `${nome}/${aba}`)
      assert.equal(barras(html), 1, `${nome}/${aba}`)
    }
  }
  // Os estados de erro continuam com a sua mensagem (nunca lista vazia) e a falha não vira zero.
  assert.match(render({ estado: { kind: 'falha', falha: 'falha' } }), /Não foi possível ler o custo agora\./)
  assert.match(render({ estado: { kind: 'falha', falha: 'falha' } }), /Tentar de novo/)
  assert.match(render({ estado: { kind: 'falha', falha: 'soDono' } }), /Só o dono deste computador vê o custo\./)
  assert.match(render({ estado: { kind: 'pronto', relatorio: vazio as never } }), /Nada registrado neste mês\./)
  assert.match(render({ estado: { kind: 'pronto', relatorio: relatorio({ contas: null, modelos: null }) as never } }), /Atualize o Hub para ver o uso por conta e por modelo\./)
  assert.match(render({ estado: { kind: 'lendo' }, online: false }), /Sem conexão com este computador/)
  // Sem nenhum limite conhecido, a seção diz isso em vez de sumir.
  assert.match(render({ limites: [] }), /Limites do plano[\s\S]*Ainda não há informação de uso\./)
})

test('"(parcial)" vem junto ao valor; sem preço nunca vira zero; moedas separadas', () => {
  const html = render({ aba: 'bot', bot: () => undefined })
  assert.match(html, /US\$\s9,00<\/span><span[^>]*>\(parcial\)<\/span>/)
  const semPreco = render({ aba: 'modelo' })
  assert.match(semPreco, /Modelo não identificado/)
  assert.doesNotMatch(semPreco, /US\$\s0,00/)
  const duas = relatorio({ bots: [{ botId: 'b1', name: 'Berga', totals: totais({ costs: [...usd(1.5), { amount: 3, currency: 'BRL', source: 'motor' }] }) }] })
  assert.match(render({ aba: 'bot', estado: { kind: 'pronto', relatorio: duas as never } }), /R\$\s3,00 \+ US\$\s1,50/)
})

test('conta: rosto e selo separados do nome; sem cadastro, ID curto e aviso (os números ficam)', () => {
  const html = render({ aba: 'conta' })
  assert.match(html, /funcao-etiqueta[^>]*>Assinatura</)
  assert.match(html, /Conta não identificada/)
  assert.match(html, /data-rosto="personagem"[\s\S]*funcao-etiqueta/, 'a conta nomeada tem rosto antes do nome e o selo depois')
  const semNomes = render({ aba: 'conta', cadastro: { kind: 'falha' } })
  assert.match(semNomes, /Conta \S*acc-1\S*/)
  assert.match(semNomes, /Não foi possível ler os nomes das contas/)
  assert.doesNotMatch(semNomes, /funcao-etiqueta/)
  assert.match(semNomes, /US\$\s2,00/)
})

test('tela 01: a seção é "Contas de IA" e tokens/turnos ficam sob o dinheiro à direita, não sob o nome', () => {
  const html = render({ aba: 'conta' })
  assert.match(html, /<h\d[^>]*>Contas de IA</)
  assert.doesNotMatch(html, /<h\d[^>]*>Por conta</)
  const linhas = html.split('<div class="value-row">').slice(1)
  const linha = linhas.find((l) => l.includes('US$ 2,00') || /US\$\s2,00/.test(l)) ?? ''
  const [esquerda, direita] = linha.split('value-row-value')
  assert.ok(direita, 'a linha tem a coluna do valor')
  assert.doesNotMatch(esquerda!, /detail-note/, 'nada de detalhe sob o nome')
  assert.match(direita!, /US\$\s2,00[\s\S]*detail-note[^>]*>Turnos: 4 · Entrada/)
  // Por bot (tela 03) segue com o detalhe sob o nome.
  const bot = render({ aba: 'bot' })
  const [nome] = (bot.split('<div class="value-row">')[1] ?? '').split('value-row-value')
  assert.match(nome!, /detail-note/)
})

test('bot: rosto e selo de categoria vindos do cadastro de bots; sem cadastro só o nome', () => {
  const bot = { id: 'b1', name: 'Berga', kind: 'bot', title: 'Principal', avatar: undefined } as never
  const com = render({ aba: 'bot', bot: () => bot })
  assert.match(com, /data-rosto="bot"[\s\S]*funcao-etiqueta[^>]*>Principal</)
  const sem = render({ aba: 'bot', bot: () => undefined })
  assert.doesNotMatch(sem, /funcao-etiqueta|data-rosto="bot"/)
  assert.match(sem, /Berga/)
})

test('árabe (RTL): nome de conta e de bot isolados (FSI…PDI), códigos de modelo e IDs em LTR (LRI…PDI)', () => {
  const FSI = '⁨', LRI = '⁦', PDI = '⁩'
  const rel = relatorio({ bots: [{ botId: 'b1', name: 'برجا Berga', totals: totais() }], modelos: [{ model: 'claude-opus-5-5', totals: totais() }, { model: null, totals: totais({ turns: 6 }) }] })
  const estado = { kind: 'pronto', relatorio: rel as never } as const
  assert.ok(render({ aba: 'bot', estado }, 'ar-AE').includes(`${FSI}برجا Berga${PDI}`))
  assert.ok(render({ aba: 'modelo', estado }, 'ar-AE').includes(`${LRI}claude-opus-5-5${PDI}`))
  const conta = render({ aba: 'conta', estado, cadastro: { kind: 'pronto', contas: [{ id: 'acc-1', label: 'حساب Claude', backend: 'claude', billing: 'subscription' }] } }, 'ar-AE')
  assert.ok(conta.includes(`${FSI}حساب Claude${PDI}`))
  assert.match(render({ aba: 'conta', estado, cadastro: { kind: 'falha' } }, 'ar-AE'), new RegExp(`${LRI}acc-1${PDI}`))
  // A tela fala árabe de fato (e não o pt-BR de antes): o título do resumo vem do catálogo ar-AE.
  assert.match(render({ aba: 'bot', estado }, 'ar-AE'), /ملخص الشهر/)
})

// MX-245: cada janela mostra o percentual e quando volta (só o que o provedor informou); Grok só se o Hub o mandar.
const provedor = (id: string, name: string, windows: object[]) => ({ id, name, windows, source: 'x', updatedAt: 1 })
const comLimites = (...providers: object[]) => ({ limites: [{ chave: 'mac', rotulo: null, usage: { providers } }] }) as Partial<PainelProps>
const dias = (n: number) => Date.now() + n * 86_400_000

test('limites do plano: cada janela mostra a hora local em que volta, no idioma do app', () => {
  const html = render(comLimites(provedor('claude', 'Claude', [
    { id: 'five_hour', label: '5-hour', percent: 42, resetsAt: dias(30) },
    { id: 'seven_day', label: 'Weekly', percent: 70, resetsText: 'quando o provedor liberar' },
    { id: 'seven_day_opus', label: 'Weekly · Opus', percent: 5 },
  ])))
  assert.match(html, /volta \d{2}\/\d{2} às \d{2}:\d{2}/)
  assert.match(html, /volta: quando o provedor liberar/, 'texto do provedor como veio')
  assert.match(html, /o provedor não informa quando volta/, 'sem informação, não inventa')
  assert.doesNotMatch(html, /renova/i, 'o "renova em" fixo em português saiu')
  assert.match(render(comLimites(provedor('claude', 'Claude', [{ id: 'f', label: '5-hour', percent: 1, resetsAt: dias(30) }])), 'en'), /back \d{2}\/\d{2} at \d{2}:\d{2} [AP]M/)
  assert.match(render(comLimites(provedor('claude', 'Claude', [{ id: 'f', label: '5-hour', percent: 1 }])), 'zh-CN'), /提供方未告知何时恢复/)
})

test('Grok só aparece se o Hub o mandar em usage.providers; sem ele, nada e nenhum zero', () => {
  const claude = provedor('claude', 'Claude', [{ id: 'f', label: '5-hour', percent: 42, resetsAt: dias(30) }])
  const sem = render(comLimites(claude))
  assert.doesNotMatch(sem, /Grok/)
  assert.equal(barras(sem), 1)
  const com = render(comLimites(claude, provedor('grok', 'Grok', [{ id: 'f', label: '5-hour', percent: 10 }])))
  assert.match(com, /Grok/)
  assert.equal(barras(com), 2)
  // Sem nenhum provedor, a seção diz que não há informação (não mostra Grok nem zero).
  assert.doesNotMatch(render({ limites: [] }), /Grok|0%/)
})


test('estimativas aparecem separadas do motor em todas as abas e sem inventar preço', () => {
  const estimado = [{ amount: 1.25, currency: 'USD', source: 'estimado' }]
  const t = totais({ costs: usd(2), estimatedCosts: estimado, unpricedTurns: 1 })
  const r = relatorio({ total: t, bots: [{ botId: 'b1', name: 'Berga', totals: t }], contas: [{ accountId: null, totals: t }], modelos: [{ model: null, totals: t }] })
  for (const aba of ['conta', 'modelo', 'bot'] as const) {
    const html = render({ aba, estado: { kind: 'pronto', relatorio: r as never } })
    assert.match(html, /US\$\s2,00/)
    assert.match(html, /Estimado: US\$\s1,25/)
    assert.match(html, /parcial/)
  }
  const soEstimado = relatorio({ total: totais({ estimatedCosts: estimado }), bots: [], contas: [], modelos: [] })
  const html = render({ estado: { kind: 'pronto', relatorio: soEstimado as never } })
  assert.match(html, /Estimado: US\$\s1,25/)
  assert.doesNotMatch(html, /Sem preço/)
})
