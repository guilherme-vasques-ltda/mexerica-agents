import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { falhaDeAjuste, falhaDeControle, hubAntigo, lerConfig, motivoEncerrada, NUNCA, opcoesDias, pedidoSalvar } from './arquivar/controle.ts'

// DON-907 C5 (D70): o ajuste "Arquivar sozinho" lido e gravado com a revisão; a conversa encerrada só para leitura,
// com o motivo e o "Continuar" só quando o Hub anuncia; nenhum texto do Hub na tela.

test('ajuste: lê a forma do Hub, "Nunca" = null, grava com expectedRevision; conflito e recusa viram código', () => {
  assert.deepEqual(lerConfig({ config: { days: 3, revision: 0, defaultDays: 3, maxDays: 90 } }), { days: 3, revision: 0 })
  assert.deepEqual(lerConfig({ config: { days: null, revision: 7 } }), { days: null, revision: 7 })
  for (const bad of [null, {}, { config: { days: 0, revision: 1 } }, { config: { days: 91, revision: 1 } }, { config: { days: '3', revision: 1 } }, { config: { days: 3 } }]) assert.throws(() => lerConfig(bad), JSON.stringify(bad))
  assert.deepEqual(pedidoSalvar(NUNCA, { days: 3, revision: 4 }), { days: null, expectedRevision: 4 })
  assert.deepEqual(pedidoSalvar(14, { days: null, revision: 5 }), { days: 14, expectedRevision: 5 })
  assert.deepEqual(opcoesDias(3), [1, 3, 7, 14, 30, 90, NUNCA])
  assert.deepEqual(opcoesDias(5), [1, 3, 5, 7, 14, 30, 90, NUNCA], 'valor de outro cliente aparece sem ser trocado')
  assert.equal(falhaDeAjuste(Object.assign(new Error('autoclose_revision_conflict'), { status: 409 })), 'conflict')
  assert.equal(falhaDeAjuste(new Error('not_allowed: este aparelho não tem a autorização do dono para esta mudança.')), 'notAllowed')
  assert.equal(falhaDeAjuste({ status: 403, message: 'forbidden' }), 'notAllowed')
  assert.equal(falhaDeAjuste(new Error('invalid_field: days')), 'failed')
  assert.equal(hubAntigo({ status: 404, message: 'unknown method sessionAutoCloseConfig' }), true)
  assert.equal(hubAntigo(new Error('falhou')), false)
})

test('encerrar/continuar: os códigos do contrato; o resto do texto do Hub nunca é usado', () => {
  const cases: [string, string][] = [
    ['session_control_pending', 'pending'], ['session_closed', 'alreadyClosed'], ['session_open', 'stillOpen'],
    ['session_revision_conflict', 'conflict'], ['not_allowed: autoridade encerrada', 'notAllowed'],
    ['session_chat_unsupported', 'chatUnsupported'], ['unknown_session', 'unknownSession'],
    ['principal_not_closable', 'failed'], ['principal_not_continuable', 'failed'], ['Texto livre do motor', 'failed'],
  ]
  for (const [message, falha] of cases) assert.equal(falhaDeControle(new Error(message)), falha, message)
  assert.equal(falhaDeControle({ status: 0, message: 'offline' }), 'offline')
  assert.equal(motivoEncerrada({ closedReason: 'idle' }), 'idle')
  assert.equal(motivoEncerrada({ closedReason: 'owner' }), 'owner')
  assert.equal(motivoEncerrada({ closedReason: 'arquivada' }), 'other')
})

// O render real do aviso, com um modelo de sessões fictício (só o que a view lê).
const bundle = await build({
  stdin: {
    contents: `export { ConversaEncerrada } from './views/fork/conversa/arquivar/Encerrada';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }, { name: 'modelo', setup(b) {
    // Sem DOM não há `getServerSnapshot`: o modelo fictício já está no estado a mostrar.
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'modelo', namespace: 'modelo' }))
    b.onLoad({ filter: /.*/, namespace: 'modelo' }, () => ({ contents: 'export class Observable {}; export const useModel = (m) => m', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, location: { search: '' } }
// O texto conferido é o pt-BR: idioma escolhido pt-BR (sem escolha, o padrão é o inglês, D454/D495).
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask })
const m = module.exports as {
  ConversaEncerrada: typeof import('./arquivar/Encerrada').ConversaEncerrada
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const chat = { id: 'byte', name: 'Byte', kind: 'bot' }
const session = (closedReason: string) => ({ id: 's-old', chatId: 'byte', principal: false, origin: { kind: 'owner' }, sourceSession: null, parentSession: null, depth: 0, title: 'Antiga', titleSource: 'owner', state: 'closed', closedReason, closedAt: 1, createdAt: 1, lastUsedAt: 1, revision: 2 })
const render = (o: { actions: string[]; busy?: boolean; reason?: string }) => {
  const sessions = { subscribe: () => () => {}, getVersion: () => 0, can: (_c: string, a: string) => o.actions.includes(a), controlBusy: () => !!o.busy, control: () => { throw new Error('render não age') } }
  const store = { spaceId: null, spaces: null, sessions }
  return m.renderToStaticMarkup(m.createElement(m.ConversaEncerrada, { store: store as never, chat: chat as never, session: session(o.reason ?? 'idle') as never, botName: 'Byte', onBack() {}, onOpen() {} }))
}

test('conversa encerrada: só leitura com o motivo; "Continuar" ao lado só com `continue`, desabilitado enquanto responde', () => {
  const idle = render({ actions: ['close', 'continue'] })
  assert.match(idle, /Encerrada por falta de atividade\./)
  assert.match(idle, /Agora é só para ler\. Byte continua lembrando/)
  assert.match(idle, /<button class="arquivar-continuar">Continuar em nova conversa<\/button>/)
  assert.doesNotMatch(idle, /<textarea|contenteditable/, 'sem campo de envio')
  assert.match(render({ actions: ['continue'], reason: 'owner' }), /Encerrada por você\./)
  assert.match(render({ actions: ['continue'], reason: 'arquivada' }), /Conversa encerrada\./)
  // Hub sem `continue`: nenhuma ação de continuar (nem substituto).
  const antigo = render({ actions: ['close'] })
  assert.doesNotMatch(antigo, /Continuar/)
  assert.match(antigo, /Voltar à principal/)
  const busy = render({ actions: ['continue'], busy: true })
  assert.match(busy, /<button class="arquivar-continuar" disabled="" aria-busy="true">/)
  assert.match(busy, /Abrindo…/)
})

test('ligação: encerrada troca o compositor pelo aviso; encerrar fica no menu da aba e no Ajustes ao lado dos Limites', () => {
  const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
  const thread = fonte('../../thread/ThreadView.tsx')
  // O ramo da encerrada vem antes do compositor: nela não há Composer.
  assert.match(thread, /openRow\?\.state === 'closed' && extras \? \(\s*<ConversaEncerrada /)
  assert.match(thread, /onTabMenu=\{encerrar\.abrir\}/)
  assert.match(fonte('../abas/ChatTabs.tsx'), /onContextMenu=\{onTabMenu \?/)
  assert.match(fonte('./ConversaSettings.tsx'), /<LimitesDeUsoSection \/>[\s\S]*<ArquivarSozinhoSection \/>[\s\S]*<ContextoAjuste \/>/)
  // Encerrar só para extra aberta, com `close` anunciado; a principal fica fora.
  assert.match(fonte('./arquivar/Encerrada.tsx'), /controller\.archive\(confirmar\)/)
})
