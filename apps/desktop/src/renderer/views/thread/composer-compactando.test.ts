import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// "Compactando…" (opção A, aprovada pelo dono): enquanto o bot compacta, o campo é IMUTÁVEL e o rascunho (texto e
// anexos) fica intacto por TODOS os caminhos de entrada. O teste usa o Composer de verdade (esbuild + render no servidor)
// e dispara os handlers reais que o React recebeu (onChange, onPaste, onKeyDown, onDrop, cliques), contra a loja e os
// rascunhos reais. Sem as proteções do Composer, estes testes falham (ver a mutação registrada no PR).
const bundle = await build({
  stdin: {
    contents: `export { Composer } from './views/thread/Composer';
      export { StoreContext } from './store/context';
      export { BotStore } from './store/bot-store';
      export { modeloDe } from './views/fork/contexto/BarraContexto';
      export { normalizeBot } from '../shared/models';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'composer', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'observavel', namespace: 'observavel' }))
    const real = createRequire(import.meta.url).resolve('react')
    const jsx = createRequire(import.meta.url).resolve('react/jsx-runtime')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g), useLayoutEffect: (f) => { if (String(f).includes('mounted.current = true')) f() },
        // Estado e refs DO COMPOSITOR sobrevivem entre os renders de uma cena (como no React de verdade, onde o componente continua o mesmo). Os hooks do DestinationComposer rodam todos antes do primeiro elemento que ele cria (o render é em profundidade); depois disso, os dos filhos são os normais.
        useState: (init) => { if (!__persistente.aberto) return R.useState(init); const e = __persistente.slots, i = __persistente.n++; if (!(i in e)) e[i] = { v: typeof init === 'function' ? init() : init }; const c = e[i]; return [c.v, (x) => { c.v = typeof x === 'function' ? x(c.v) : x }] },
        useRef: (init) => { if (!__persistente.aberto) return R.useRef(init); const e = __persistente.slots, i = __persistente.n++; if (!(i in e)) e[i] = { ref: { current: init } }; return e[i].ref }, useCallback: (f, deps) => { if (!__persistente.aberto) return R.useCallback(f, deps); const e = __persistente.slots, i = __persistente.n++; const antes = e[i]; if (antes && antes.deps.length === deps.length && deps.every((d, k) => Object.is(d, antes.deps[k]))) return antes.v; e[i] = { deps, v: f }; return f },
        useMemo: (f, deps) => { if (!__persistente.aberto) return R.useMemo(f, deps); const e = __persistente.slots, i = __persistente.n++; const antes = e[i]; if (antes && antes.deps.length === deps.length && deps.every((d, k) => Object.is(d, antes.deps[k]))) return antes.v; const v = f(); e[i] = { deps, v }; return v },
        // Efeitos do compositor rodam de verdade, DEPOIS do render (como o commit do React), com troca de dependências e limpeza.
        useEffect: (f, deps) => {
          if (!__persistente.aberto) return
          const e = __persistente.slots, i = __persistente.n++
          const antes = e[i]
          if (antes && deps && antes.deps && deps.length === antes.deps.length && deps.every((d, k) => Object.is(d, antes.deps[k]))) return
          if (!antes) e[i] = { efeito: true, deps: undefined, limpar: undefined }
          __persistente.fila.push(() => { const x = e[i]; if (x.limpar) x.limpar(); x.deps = deps; x.limpar = f() })
        } }` }))
    // Cada elemento criado pelo app passa por aqui: guarda as props (os handlers) dos controles do compositor.
    b.onResolve({ filter: /^react\/jsx-runtime$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'jsx-shim' ? { path: jsx } : { path: 'jsx', namespace: 'jsx-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'jsx-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(jsx)});
      exports.Fragment = R.Fragment;
      exports.jsx = (t, p, k) => { __registrar(t, p); return R.jsx(t, p, k) };
      exports.jsxs = (t, p, k) => { __registrar(t, p); return R.jsxs(t, p, k) };` }))
    b.onLoad({ filter: /.*/, namespace: 'observavel' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }\nexport const useModel = (m) => m\nexport const useModels = () => {}\nexport const useSelector = (m, select) => select(m)\nexport const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]))\nexport const useSelectMany = (ms, f) => f()\nexport const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
  logLevel: 'silent',
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
/** Relógio manual do app inteiro dentro do teste: `setTimeout` e `Date.now` só avançam quando o teste manda. */
const reloj = (() => {
  let agora = 1_700_000_000_000
  const tarefas = new Map<number, { f: () => void; em: number }>()
  let id = 0
  return {
    agora: () => agora,
    agendar: (f: () => void, ms = 0) => { tarefas.set(++id, { f, em: agora + ms }); return id },
    cancelar: (n: number) => void tarefas.delete(n),
    passar: (ms: number) => { agora += ms; for (const [n, t] of [...tarefas]) if (t.em <= agora) { tarefas.delete(n); t.f() } },
  }
})()
class DataManual extends Date { static override now() { return reloj.agora() } }
const ditado: { ouvinte: ((e: any) => void) | null } = { ouvinte: null }
const lidos: { arquivos: number } = { arquivos: 0 }
const window = { codync: { platform: 'linux', speech: { available: true, onEvent: (f: (e: any) => void) => { ditado.ouvinte = f; return () => { ditado.ouvinte = null } }, start() {}, stop() {} }, app: {
  pickFiles: async () => [{ name: 'a.txt', data: new Uint8Array([1]) }],
  readClipboardFiles: async () => { lidos.arquivos++; return [{ name: 'colado.png', data: new Uint8Array([2]) }] },
} }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, location: { search: '' } }
const no = () => {}
const element = () => ({ setAttribute: no, removeAttribute: no, getAttribute: () => null, style: {}, classList: { add: no, remove: no, toggle: no, contains: () => false }, addEventListener: no, removeEventListener: no, appendChild: no, querySelector: () => null, querySelectorAll: () => [], getBoundingClientRect: () => ({ top: 0, height: 0 }) })
const document = { documentElement: { ...element(), lang: '', dir: '' }, body: element(), head: element(), createElement: element, addEventListener: no, removeEventListener: no, querySelector: () => null, querySelectorAll: () => [], visibilityState: 'visible', hasFocus: () => true }
/* eslint-disable @typescript-eslint/no-explicit-any */
type Props = Record<string, any>
const vistos: { tipo: unknown; props: Props }[] = []
const __persistente: { slots: Record<number, any>; n: number; aberto: boolean; fila: (() => void)[] } = { slots: {}, n: 0, aberto: false, fila: [] }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { document, module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage,
  navigator: { platform: 'Linux', language: 'pt-BR', languages: ['pt-BR'] }, TextEncoder, TextDecoder, URL, URLSearchParams, console, setTimeout: reloj.agendar, clearTimeout: reloj.cancelar, setInterval: () => 0, clearInterval: () => {}, Date: DataManual, queueMicrotask, crypto, requestAnimationFrame: () => 0,
  __registrar: (tipo: unknown, props: Props) => {
    // O compositor de verdade é o `DestinationComposer`: do seu primeiro hook ao primeiro elemento que ele cria.
    const ele = typeof tipo === 'function' && (tipo as { name?: string }).name === 'DestinationComposer'
    __persistente.aberto = ele
    if (ele) __persistente.n = 0
    vistos.push({ tipo, props })
  },
  __persistente,
  __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } } })
const m = module.exports as any
const h = m.createElement

const nenhum = (): never => { throw new Error('sem Hub no teste') }

interface Cena { desmontar: () => void; slots: Record<number, any>; store: any; botId: string; modelo: any; render: () => string; campo: () => Props; controles: (pred: (p: Props) => boolean) => Props[]; chamadas: string[] }

/** Uma conversa aberta no Composer real; `compactando` liga a atividade "resumindo" do bot, sob o relógio manual. */
function cena({ compactando, grupo = false }: { compactando: boolean; grupo?: boolean }): Cena {
  const store = new m.BotStore({ id: 'c', name: 'C', signKey: '', urls: [] }, true, nenhum, '2.7.0', 'ctx', 'personal')
  store.connection = { kind: 'online' }
  const botId = grupo ? 'g' : 'b'
  const membro = m.normalizeBot({ id: 'm1', name: 'Mia', kind: 'agent' })
  store.bots.set('m1', membro)
  const bot = grupo ? m.normalizeBot({ id: 'g', name: 'Canal', kind: 'group', members: ['m1'] }) : m.normalizeBot({ id: 'b', name: 'Bia', kind: 'agent', fastModeSupport: 'supported' })
  store.bots.set(botId, bot)
  const modelo = m.modeloDe(store)
  const chamadas: string[] = []
  store.setFastMode = (b: any) => { chamadas.push(`fast:${b.id}`) }
  store.send = () => { chamadas.push('send'); return {} }
  const cenaAtual: Cena = {
    slots: {}, desmontar: () => { for (const x of Object.values(cenaAtual.slots)) if (x.efeito && typeof x.limpar === 'function') x.limpar() }, store, botId, modelo, chamadas,
    render: () => { vistos.length = 0; __persistente.slots = cenaAtual.slots; __persistente.n = 0; __persistente.aberto = false; __persistente.fila = []; const html = m.renderToStaticMarkup(h(m.StoreContext.Provider, { value: store }, h(m.Composer, { botId }))); for (const efeito of __persistente.fila) efeito(); return html },
    campo: () => { cenaAtual.render(); const t = vistos.filter((v) => v.tipo === 'textarea').at(-1); assert.ok(t, 'o campo foi desenhado'); return t.props },
    controles: (pred) => { cenaAtual.render(); return vistos.filter((v) => v.tipo === 'button' && pred(v.props)).map((v) => v.props) },
  }
  if (compactando) {
    store.bots.set(botId, { ...bot, status: 'working', workingChat: botId, activityCode: { kind: 'summarizing' } })
  }
  return cenaAtual
}

const evento = (extra: Props = {}) => {
  const e: Props = { key: '', shiftKey: false, altKey: false, metaKey: false, ctrlKey: false, repeat: false, keyCode: 0, nativeEvent: { isComposing: false }, defaultPrevented: false,
    preventDefault() { e.defaultPrevented = true }, stopPropagation() {}, currentTarget: { selectionStart: 0, selectionEnd: 0 }, ...extra }
  return e
}
const enter = () => evento({ key: 'Enter', keyCode: 13 })
const colagem = (texto: string | null, arquivos: unknown[] = []) => evento({ clipboardData: { types: [...(texto !== null ? ['text/plain'] : []), ...(arquivos.length ? ['Files'] : [])], files: arquivos, getData: () => texto ?? '' } })
const soltar = (arquivos: unknown[], texto = '') => evento({ dataTransfer: { types: arquivos.length ? ['Files'] : ['text/plain'], files: arquivos, getData: () => texto } })
const arquivo = (nome: string) => ({ name: nome, arrayBuffer: async () => new Uint8Array([9]).buffer })
const chaveSecreta = ['sk', '_live_', 'Qw3Er5Ty7Ui9Op1As3Df5Gh7'].join('')
const nomes = (arquivos: any[]) => Array.from(arquivos, (f) => f.name as string)
const tick = () => new Promise((r) => setTimeout(r, 0))
const RASCUNHO = 'E o aditivo de prazo, quem precisa assinar?'

test('compactando: o campo é só leitura, com o motivo; Enviar, Anexar e Ditar desligados', () => {
  const c = cena({ compactando: true })
  c.store.composerDrafts.edit('b', RASCUNHO, 'typed')
  const html = c.render()
  assert.match(html, /<textarea[^>]*readOnly=""|<textarea[^>]*readonly=""/i)
  assert.match(html, /aria-readonly="true"/)
  assert.match(html, /Compactando a conversa\. Seu texto fica guardado/)
  const enviar = c.controles((p) => p['aria-label'] === 'Enviar')[0]!
  assert.equal(enviar.disabled, true)
  assert.equal(c.controles((p) => String(p.className).includes('composer-add'))[0]!.disabled, true)
  // Sem compactar, o mesmo campo é livre (controle: o teste enxerga a diferença).
  const livre = cena({ compactando: false })
  assert.doesNotMatch(livre.render(), /aria-readonly/)
})

test('compactando: digitar, colar (texto, senha, arquivo), soltar e Enter não mudam o rascunho nem enviam', async () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  d.edit('b', RASCUNHO, 'typed')
  const campo = () => c.campo()
  // digitar
  campo().onChange({ target: { value: RASCUNHO + ' xxx' } })
  assert.equal(d.get('b'), RASCUNHO, 'digitar')
  // colar texto comum: o navegador não pode inserir
  const t = colagem('texto novo')
  campo().onPaste(t)
  assert.equal(t.defaultPrevented, true, 'colar texto é barrado')
  // colar o que parece senha: nada vai ao rascunho e nada fica em quarentena
  const s = colagem(chaveSecreta)
  campo().onPaste(s)
  assert.equal(s.defaultPrevented, true, 'colar senha é barrado')
  assert.equal(d.get('b'), RASCUNHO, 'colar senha')
  // colar arquivo/imagem
  const a = colagem(null, [arquivo('foto.png')])
  campo().onPaste(a)
  await tick(); await tick()
  assert.equal(a.defaultPrevented, true, 'colar arquivo é barrado')
  assert.equal(d.files('b').length, 0, 'colar arquivo')
  assert.equal(lidos.arquivos, 0, 'nem lê a área de transferência')
  // soltar arquivo e texto
  const alvo = c.controles(() => false) // força um render
  void alvo
  const caixa = vistos.find((v) => v.tipo === 'div' && String(v.props.className).startsWith('composer idioma'))!.props
  const solto = soltar([arquivo('a.txt')])
  caixa.onDrop(solto)
  await tick(); await tick()
  assert.equal(solto.defaultPrevented, true)
  assert.equal(d.files('b').length, 0, 'soltar arquivo')
  const sobre = soltar([arquivo('a.txt')])
  caixa.onDragOver(sobre)
  assert.equal(sobre.defaultPrevented, false, 'o campo não aceita soltar')
  // Enter: não envia, não apaga
  const e = enter()
  campo().onKeyDown(e)
  assert.equal(e.defaultPrevented, true)
  assert.equal(c.chamadas.includes('send'), false, 'Enter não envia')
  assert.equal(d.get('b'), RASCUNHO, 'Enter')
  const botao = c.controles((p) => p['aria-label'] === 'Enviar')[0]!
  botao.onClick?.()
  assert.equal(c.chamadas.includes('send'), false, 'clicar em Enviar não envia')
  assert.equal(d.get('b'), RASCUNHO)
})

test('compactando: comando "/fast" no rascunho não roda nem limpa (Enter, clique e envio)', () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  d.edit('b', '/fast', 'typed')
  const e = enter()
  c.campo().onKeyDown(e)
  assert.equal(d.get('b'), '/fast', 'o menu de comandos não processa o Enter')
  assert.deepEqual(c.chamadas, [], 'o comando não rodou')
  c.controles((p) => p['aria-label'] === 'Enviar')[0]!.onClick?.()
  assert.deepEqual(c.chamadas, [])
  assert.equal(d.get('b'), '/fast')
  // Controle: sem compactar, o mesmo Enter roda o comando e limpa o campo (a proteção é o que muda isso).
  const livre = cena({ compactando: false })
  livre.store.composerDrafts.edit('b', '/fast', 'typed')
  livre.campo().onKeyDown(enter())
  assert.deepEqual(livre.chamadas, ['fast:b'])
  assert.equal(livre.store.composerDrafts.get('b'), '')
})

test('compactando: menção a bot do canal não altera o rascunho', () => {
  const c = cena({ compactando: true, grupo: true })
  const d = c.store.composerDrafts
  d.edit('g', 'oi @', 'typed')
  assert.equal(c.controles((p) => String(p.className).includes('mention')).length, 0, 'sem sugestões enquanto compacta')
  // Controle: livre, a sugestão existe e escreve no rascunho.
  const livre = cena({ compactando: false, grupo: true })
  livre.store.composerDrafts.edit('g', 'oi @', 'typed')
  const sugestao = livre.controles((p) => String(p.className).includes('mention'))[0]!
  sugestao.onClick()
  assert.equal(livre.store.composerDrafts.get('g'), 'oi @Mia ')
})

test('compactando: anexo que volta do seletor não entra e remover anexo não mexe nos anexos', async () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  const velho = { id: 'f1', name: 'velho.txt', data: new Uint8Array([1]) }
  d.setFiles('b', [velho])
  // O botão está desligado, mas o seletor pode já ter sido aberto: o que ele devolve é recusado.
  await c.controles((p) => String(p.className).includes('composer-add'))[0]!.onClick()
  await tick(); await tick()
  assert.deepEqual(nomes(d.files('b')), ['velho.txt'], 'o anexo tardio não entra')
  // Remover um anexo também é alterar o rascunho.
  c.render()
  vistos.find((v) => typeof v.tipo === 'function' && v.props.file === velho)!.props.remove()
  assert.deepEqual(nomes(d.files('b')), ['velho.txt'], 'remover anexo não vale')
  // Controle: livre, o mesmo seletor adiciona e o chip remove.
  const livre = cena({ compactando: false })
  livre.store.composerDrafts.setFiles('b', [velho])
  await livre.controles((p) => String(p.className).includes('composer-add'))[0]!.onClick()
  await tick(); await tick()
  assert.deepEqual(nomes(livre.store.composerDrafts.files('b')), ['velho.txt', 'a.txt'])
  livre.render()
  vistos.find((v) => typeof v.tipo === 'function' && v.props.file === velho)!.props.remove()
  assert.deepEqual(nomes(livre.store.composerDrafts.files('b')), ['a.txt'])
})

test('compactando: "Experimentar" de uma rotina não escreve no rascunho (e o texto espera, sem se perder)', () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  d.edit('b', RASCUNHO, 'typed')
  c.store.routineDrafts.set('b', 'texto da rotina')
  c.render() // o efeito do Composer que consome o rascunho da rotina
  assert.equal(d.get('b'), RASCUNHO, 'o rascunho não muda')
  assert.equal(c.store.routineDrafts.get('b'), 'texto da rotina', 'o texto da rotina continua esperando')
  // Controle: livre, o mesmo efeito acrescenta o texto da rotina.
  const livre = cena({ compactando: false })
  livre.store.composerDrafts.edit('b', RASCUNHO, 'typed')
  livre.store.routineDrafts.set('b', 'texto da rotina')
  livre.render()
  assert.equal(livre.store.composerDrafts.get('b'), `${RASCUNHO}\ntexto da rotina`)
})

const compactandoAgora = (c: Cena) => c.campo()['aria-readonly'] === true
const ficaOciosoBot = (c: Cena) => c.store.bots.set('b', { ...c.store.bots.get('b'), status: 'idle', workingChat: null, activityCode: null })
const comecaCompactar = (c: Cena) => c.store.bots.set('b', { ...c.store.bots.get('b'), status: 'working', workingChat: 'b', activityCode: { kind: 'summarizing' } })
const aviso = (c: Cena) => { c.render(); return vistos.find((v) => typeof v.tipo === 'function' && 'naoESenha' in v.props)?.props }

test('o limite de 90 s libera o campo e o envio, mesmo depois de trocar de conexão; o relógio é um só', () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  d.edit('b', RASCUNHO, 'typed')
  assert.equal(compactandoAgora(c), true) // o efeito do Composer passa a observar a atividade
  reloj.passar(60_000)
  c.modelo.descartar('b', null) // BarraContexto descarta a conversa quando a conexão ou o Hub muda
  assert.equal(compactandoAgora(c), true, 'trocar de conexão não solta nem reinicia')
  reloj.passar(29_000)
  assert.equal(compactandoAgora(c), true, '89 s: ainda esperando')
  reloj.passar(1_000)
  assert.equal(compactandoAgora(c), false, '90 s contados do começo: o campo volta')
  // O rascunho preservado agora pode ser enviado.
  assert.equal(d.get('b'), RASCUNHO)
  c.campo().onKeyDown(enter())
  assert.deepEqual(c.chamadas, ['send'])
})

test('desmontar a tela solta o envio (o efeito do Composer limpa o registro)', () => {
  const c = cena({ compactando: true })
  assert.equal(compactandoAgora(c), true)
  assert.equal(c.modelo.compactacao('b', null)?.fase, 'andamento')
  c.desmontar()
  assert.equal(c.modelo.compactacao('b', null), null)
})

test('terminar a compactação devolve o campo com o rascunho intacto, e nada é enviado sozinho', () => {
  const c = cena({ compactando: true })
  const d = c.store.composerDrafts
  d.edit('b', RASCUNHO, 'typed')
  assert.equal(compactandoAgora(c), true)
  c.campo().onKeyDown(enter())
  assert.equal(d.get('b'), RASCUNHO)
  ficaOciosoBot(c)
  assert.equal(compactandoAgora(c), false)
  assert.equal(d.get('b'), RASCUNHO, 'nada se perdeu')
  assert.deepEqual(c.chamadas, [], 'terminar não envia nada sozinho')
  c.campo().onKeyDown(enter())
  assert.deepEqual(c.chamadas, ['send'], 'o envio voltou: a pessoa envia o rascunho')
})

test('ciclo: colagem em quarentena → compactação (atividade tardia) → "Não é senha" espera → 90 s → resolve → envia', () => {
  const c = cena({ compactando: false })
  const d = c.store.composerDrafts
  d.edit('b', 'a senha é ', 'typed')
  // 1) antes de compactar: a colagem que parece senha vai à quarentena (o trecho sai do rascunho)
  c.campo().onPaste(evento({ currentTarget: { selectionStart: 10, selectionEnd: 10 }, clipboardData: { types: ['text/plain'], files: [], getData: () => chaveSecreta } }))
  assert.equal(d.get('b'), 'a senha é ', 'o trecho não entra no rascunho')
  assert.ok(aviso(c), 'o aviso de quarentena está aberto')
  // 2) a compactação começa; 60 s depois, outra tela passa a observar a mesma atividade (atividade tardia)
  comecaCompactar(c)
  assert.equal(compactandoAgora(c), true)
  reloj.passar(60_000)
  const tardia = c.modelo.observarAtividade('b', null)
  // 3) "Não é senha" durante a compactação: nada muda, a quarentena segue aberta
  aviso(c)!.naoESenha()
  assert.equal(d.get('b'), 'a senha é ', 'o rascunho não muda durante a compactação')
  assert.ok(aviso(c), 'a quarentena continua aberta, esperando')
  assert.deepEqual(c.chamadas, [])
  // 4) a atividade tardia não estende o prazo: aos 90 s do PRIMEIRO sinal o campo volta
  reloj.passar(29_000)
  assert.equal(compactandoAgora(c), true)
  reloj.passar(1_000)
  assert.equal(compactandoAgora(c), false, '90 s a partir do primeiro sinal')
  // 5) liberado, "Não é senha" resolve a quarentena e o trecho volta ao rascunho; o envio funciona
  aviso(c)!.naoESenha()
  assert.equal(d.get('b'), `a senha é ${chaveSecreta}`)
  assert.equal(aviso(c), undefined, 'quarentena encerrada')
  c.campo().onKeyDown(enter())
  assert.deepEqual(c.chamadas, ['send'])
  tardia()
})

test('ciclo: compactação termina antes dos 90 s → quarentena resolve com o rascunho intacto, sem enviar sozinho', () => {
  const c = cena({ compactando: false })
  const d = c.store.composerDrafts
  d.edit('b', 'a senha é ', 'typed')
  c.campo().onPaste(evento({ currentTarget: { selectionStart: 10, selectionEnd: 10 }, clipboardData: { types: ['text/plain'], files: [], getData: () => chaveSecreta } }))
  comecaCompactar(c)
  assert.equal(compactandoAgora(c), true)
  aviso(c)!.naoESenha()
  assert.equal(d.get('b'), 'a senha é ')
  ficaOciosoBot(c)
  assert.equal(compactandoAgora(c), false)
  assert.equal(d.get('b'), 'a senha é ', 'terminar não mexe no rascunho')
  assert.deepEqual(c.chamadas, [])
  aviso(c)!.naoESenha()
  assert.equal(d.get('b'), `a senha é ${chaveSecreta}`)
})

test('ciclo: ditado em andamento é encerrado ao compactar e, se uma palavra ainda chegar, não escreve no rascunho', () => {
  const c = cena({ compactando: false })
  const d = c.store.composerDrafts
  d.edit('b', 'Oi', 'typed')
  c.controles((p) => String(p.className).includes('composer-mic') && !String(p.className).includes('composer-call'))[0]!.onClick()
  const ouvir = ditado.ouvinte
  assert.ok(ouvir, 'o ditado começou')
  c.render()
  ouvir!({ type: 'partial', text: 'tudo bem' })
  assert.equal(d.get('b'), 'Oi tudo bem', 'livre, o ditado escreve (controle)')
  comecaCompactar(c)
  assert.equal(compactandoAgora(c), true)
  assert.equal(ditado.ouvinte, null, 'o ditado foi encerrado')
  ouvir!({ type: 'partial', text: 'palavra atrasada' })
  assert.equal(d.get('b'), 'Oi tudo bem', 'palavra atrasada não entra')
})

test('ciclo: pedido manual sem resposta + atividade tardia + aceite tardio: o prazo é absoluto desde o envio (90 s)', async () => {
  const c = cena({ compactando: false })
  const d = c.store.composerDrafts
  d.edit('b', RASCUNHO, 'typed')
  // Um Hub de mentira: lê o contexto; "Comprimir agora" só responde quando o teste manda.
  let aceitar!: (v: unknown) => void
  const client = { spaceId: null, call: async (metodo: string) => (metodo === 'donnaContext'
    ? { hubId: 'h', botId: 'b', sessionId: 'b', scopeId: 'A', epoch: 0, context: { used: 60_000, size: 200_000, updatedAt: 1, trigger: 180_000, canCompact: true, busy: false, contextId: 'c1' } }
    : new Promise((ok) => { aceitar = ok })) }
  Object.defineProperty(c.store, 'connectedClient', { get: () => client })
  c.store.hello = { hostId: 'h' }
  await c.modelo.ler('b', null)
  const pedido = c.modelo.comprimir('b', null, 'c1')
  await tick()
  assert.equal(compactandoAgora(c), true, 'o pedido manual bloqueia')
  reloj.passar(85_000)
  comecaCompactar(c) // o Hub só mostra a atividade aos 85 s (atividade tardia)
  assert.equal(compactandoAgora(c), true)
  reloj.passar(4_000)
  aceitar({ accepted: true }) // aceite tardio, aos 89 s
  await pedido
  assert.equal(compactandoAgora(c), true, '89 s: ainda esperando')
  reloj.passar(1_000)
  assert.equal(compactandoAgora(c), false, '90 s contados do envio: o campo volta (não aos 175 s nem aos 179 s)')
  assert.equal(d.get('b'), RASCUNHO)
  c.campo().onKeyDown(enter())
  assert.deepEqual(c.chamadas, ['send'])
})

test('ciclo: pedido manual que nunca responde + atividade tardia: libera aos 90 s do envio, ainda em voo', async () => {
  const c = cena({ compactando: false })
  const client = { spaceId: null, call: async (metodo: string) => (metodo === 'donnaContext'
    ? { hubId: 'h', botId: 'b', sessionId: 'b', scopeId: 'A', epoch: 0, context: { used: 60_000, size: 200_000, updatedAt: 1, trigger: 180_000, canCompact: true, busy: false, contextId: 'c1' } }
    : new Promise(() => {})) }
  Object.defineProperty(c.store, 'connectedClient', { get: () => client })
  c.store.hello = { hostId: 'h' }
  await c.modelo.ler('b', null)
  void c.modelo.comprimir('b', null, 'c1')
  await tick()
  reloj.passar(85_000)
  comecaCompactar(c)
  assert.equal(compactandoAgora(c), true, '85 s: esperando')
  reloj.passar(5_000)
  assert.equal(compactandoAgora(c), false, '90 s do envio, mesmo com a atividade tardia e sem resposta')
})
