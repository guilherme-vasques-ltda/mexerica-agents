import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import test from 'node:test'
import { build } from 'esbuild'
import { chromium, type Browser, type Page } from 'playwright-core'

// "Compactando…" no Composer REAL: React DOM de verdade, num Chromium de verdade, com relógio falso do Playwright
// (`page.clock`). Montagem, atualização e desmontagem são as do React; os eventos são eventos do DOM (colar, soltar,
// teclas, cliques); a loja, os rascunhos, o modelo de contexto e a BarraContexto são os do app. Só o Hub é de mentira
// (um cliente que responde `donnaContext` e deixa "Comprimir agora" pendurado). Sem Chromium neste ambiente, os testes
// pulam com aviso (use `CHROME=/caminho`; o Chromium do Playwright também serve). Ao rodar a suíte de uma revisão, confira que estes testes NÃO foram pulados.
const noPath = (nome: string) => (process.env.PATH ?? '').split(':').map((d) => `${d}/${nome}`).find((c) => existsSync(c))
const exe = [process.env.CHROME, '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable',
  noPath('chromium'), noPath('chromium-browser'), noPath('google-chrome'), noPath('chrome'), chromium.executablePath()]
  .find((c): c is string => !!c && existsSync(c))
const pular = exe ? false : 'sem Chromium (defina CHROME=/caminho): o teste do Composer real foi pulado'

const bundle = pular ? null : await build({
  stdin: {
    contents: `export { createRoot } from 'react-dom/client';
      export { createElement, useState } from 'react';
      export { Composer } from './views/thread/Composer';
      export { StoreContext } from './store/context';
      export { BotStore } from './store/bot-store';
      export { modeloDe } from './views/fork/contexto/BarraContexto';
      export { normalizeBot } from '../shared/models';`,
    resolveDir: new URL('../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'browser', format: 'iife', globalName: 'App', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
  logLevel: 'silent',
})

// O ambiente do app dentro da página: `window.codync` aceita qualquer chamada que o teste não tenha de saber.
const INICIO = `
  window.__importMeta = { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } };
  const sem = () => new Proxy(function () {}, { get: (_, k) => (k === 'then' ? undefined : sem()), apply: () => undefined });
  const base = { platform: 'linux', speech: { available: false }, app: { pickFiles: async () => [], readClipboardFiles: async () => [] }, forkCofre: { onEstado: () => () => {} } };
  window.codync = new Proxy(base, { get: (t, k) => (k in t ? t[k] : sem()) });
  localStorage.setItem('appLanguage', '"pt-BR"');
`
const ABRIR = `
  window.__enviados = []; window.__fast = 0;
  window.__client = { spaceId: null, call: async (metodo) => {
    if (metodo === 'donnaContext') return { hubId: 'h', botId: 'b', sessionId: 'b', scopeId: 'A', epoch: 0, context: { used: 60000, size: 200000, updatedAt: 1, trigger: 180000, canCompact: true, busy: false, contextId: 'c1' } };
    return new Promise(() => {}); // "Comprimir agora" fica sem resposta
  } };
  const store = new App.BotStore({ id: 'c', name: 'C', signKey: '', urls: [] }, true, () => { throw new Error('sem Hub') }, '2.7.0', 'ctx', 'personal');
  store.connection = { kind: 'online' };
  store.hello = { hostId: 'h' };
  Object.defineProperty(store, 'connectedClient', { get: () => window.__client });
  store.bots.set('b', App.normalizeBot({ id: 'b', name: 'Bia', kind: 'agent', fastModeSupport: 'supported' }));
  store.send = (texto) => { window.__enviados.push(texto); store.composerDrafts.edit('b', '', 'typed'); return {}; };
  store.setFastMode = () => { window.__fast++; };
  window.__store = store; window.__modelo = App.modeloDe(store);
  function Editor() {
    const [starterDraft, setStarterDraft] = App.useState(null);
    const [sessionId, setSessionId] = App.useState(null);
    window.__sugerir = prompt => setStarterDraft({ scope: 'principal', prompt });
    window.__sessao = setSessionId;
    return App.createElement(App.Composer, { botId: 'b', sessionId, starterDraft, onStarterDraftHandled: applied => {
      window.__sugestaoAplicada = applied;
      setStarterDraft(null);
    } });
  }
  window.__montar = () => { window.__raiz = App.createRoot(document.getElementById('root')); window.__raiz.render(App.createElement(App.StoreContext.Provider, { value: store }, App.createElement(Editor))); };
  window.__montar();
`
const CHAVE = ['sk', '_live_', 'Qw3Er5Ty7Ui9Op1As3Df5Gh7'].join('')
const RASCUNHO = 'E o aditivo de prazo, quem precisa assinar?'

let browser: Browser
test.before(async () => { if (!pular) browser = await chromium.launch({ executablePath: exe, args: ['--no-sandbox'] }) })
test.after(async () => { await browser?.close() })

interface Cena { page: Page; assentar: () => Promise<void>; estado: () => Promise<{ somenteLeitura: boolean; rascunho: string; enviados: string[]; fast: number; quarentena: boolean; motivo: boolean }>
  correr: (ms: number) => Promise<void>; compactando: () => Promise<void>; terminou: () => Promise<void>; trocarConexao: () => Promise<void>
  colar: (texto: string) => Promise<boolean>; enter: () => Promise<void>; comprimirAgora: () => Promise<void>; clicarNaoESenha: () => Promise<void> }

async function cena(): Promise<Cena> {
  const page = await browser.newPage()
  await page.clock.install({ time: 1_700_000_000_000 })
  await page.clock.pauseAt(1_700_000_000_000 + 1_000) // congela: só `runFor` faz o tempo andar
  await page.addInitScript(INICIO)
  await page.route('https://app.test/**', (r) => r.fulfill({ contentType: 'text/html', body: '<!doctype html><html lang="pt-BR"><body><div id="root"></div></body></html>' }))
  const erros: string[] = []
  page.on('pageerror', (e) => erros.push(e.message))
  await page.goto('https://app.test/')
  await page.addScriptTag({ content: bundle!.outputFiles[0]!.text })
  await page.evaluate(ABRIR)
  // Deixa o React e as leituras do modelo assentarem (MessageChannel não é falsificado pelo relógio).
  const assentar = async () => { for (let i = 0; i < 4; i++) await page.evaluate(() => new Promise<void>((r) => { const c = new MessageChannel(); c.port1.onmessage = () => r(); c.port2.postMessage(0) })) }
  await assentar()
  const c: Cena = {
    page, assentar,
    estado: async () => { await assentar(); const e = await page.evaluate(() => {
      const ta = document.querySelector('textarea')!
      return { somenteLeitura: ta.getAttribute('aria-readonly') === 'true', rascunho: (window as any).__store.composerDrafts.get('b'), enviados: (window as any).__enviados as string[], fast: (window as any).__fast as number,
        quarentena: [...document.querySelectorAll('button')].some((b) => b.textContent?.includes('Não é senha')), motivo: !!document.body.textContent?.includes('Compactando a conversa') }
    }); if (erros.length) throw new Error(`erro na página: ${erros[0]}`); return e },
    correr: async (ms) => { await page.clock.runFor(ms); await assentar() },
    compactando: async () => { await page.evaluate(() => { const s = (window as any).__store; s.bots.set('b', { ...s.bots.get('b'), status: 'working', workingChat: 'b', activityCode: { kind: 'summarizing' } }); s.changed() }); await assentar() },
    terminou: async () => { await page.evaluate(() => { const s = (window as any).__store; s.bots.set('b', { ...s.bots.get('b'), status: 'idle', workingChat: null, activityCode: null }); s.changed() }); await assentar() },
    trocarConexao: async () => { await page.evaluate(() => { const w = window as any; w.__client = { ...w.__client }; w.__store.principalEpoch += 1; w.__store.changed() }); await assentar() },
    colar: async (texto) => { const r = await page.evaluate((t) => { const ta = document.querySelector('textarea')!; ta.focus(); ta.setSelectionRange(ta.value.length, ta.value.length)
      const dt = new DataTransfer(); dt.setData('text/plain', t); const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }); ta.dispatchEvent(ev); return ev.defaultPrevented }, texto); await assentar(); return r },
    enter: async () => { await page.locator('textarea').press('Enter'); await assentar() },
    comprimirAgora: async () => { await page.evaluate(() => { (document.querySelector('.ctx-anel') as HTMLElement).click() }); await assentar(); await page.evaluate(() => { (document.querySelector('.ctx-comprimir') as HTMLElement).click() }); await assentar() },
    clicarNaoESenha: async () => { await page.evaluate(() => { [...document.querySelectorAll('button')].find((b) => b.textContent?.includes('Não é senha'))!.click() }); await assentar() },
  }
  await page.evaluate((t) => { (window as any).__store.composerDrafts.edit('b', t, 'typed') }, RASCUNHO)
  await assentar()
  return c
}
const fecha = async (c: Cena) => { await c.page.close() }

test('sugestão intacta é substituída no Composer real; edição, rotina e bloqueio são preservados', { skip: pular }, async () => {
  const c = await cena()
  const sugerir = async (texto: string) => {
    await c.page.evaluate((t) => (window as any).__sugerir(t), texto)
    const estado = await c.estado()
    return [estado.rascunho, await c.page.evaluate(() => (window as any).__sugestaoAplicada)]
  }
  try {
    await c.page.locator('textarea').fill('')
    assert.deepEqual(await sugerir('Primeira tarefa'), ['Primeira tarefa', true])
    assert.deepEqual(await sugerir('Segunda tarefa'), ['Segunda tarefa', true])
    await c.compactando()
    assert.deepEqual(await sugerir('Terceira tarefa'), ['Segunda tarefa', false])
    await c.terminou()
    assert.equal((await c.estado()).rascunho, 'Segunda tarefa')
    await c.page.locator('textarea').fill('Minha edição')
    assert.deepEqual(await sugerir('Outra tarefa'), ['Minha edição', false])
    await c.page.evaluate(() => (window as any).__store.setRoutineDraft('b', 'Texto da rotina'))
    const final = await c.estado()
    assert.equal(final.rascunho, 'Minha edição\nTexto da rotina')
    assert.deepEqual(final.enviados, [])
    await c.page.evaluate(() => (window as any).__sessao('extra'))
    await c.assentar()
    assert.deepEqual(await sugerir('Tarefa fora da conversa principal'), [final.rascunho, false])
    assert.equal(await c.page.locator('textarea').inputValue(), '')
  } finally { await fecha(c) }
})

test('sequência da revisão no Composer real: t=0 pedido, t=85 atividade, troca de conexão, t=90 libera; não volta a bloquear', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.comprimirAgora() // t = 0: "Comprimir agora" pelo balão, sem resposta do Hub
    assert.equal((await c.estado()).somenteLeitura, true, 'o pedido bloqueia o campo')
    assert.equal((await c.estado()).motivo, true, 'o motivo aparece')
    await c.correr(85_000)
    await c.compactando() // t = 85: o Hub só mostra a atividade agora
    await c.trocarConexao() // troca de conexão: a BarraContexto descarta a conversa
    assert.equal((await c.estado()).somenteLeitura, true, '85 s: ainda esperando')
    await c.correr(4_999)
    assert.equal((await c.estado()).somenteLeitura, true, '89,999 s')
    await c.correr(1)
    const livre = await c.estado()
    assert.equal(livre.somenteLeitura, false, '90 s do primeiro sinal: o campo volta')
    assert.equal(livre.rascunho, RASCUNHO, 'o rascunho está intacto')
    await c.correr(85_000)
    assert.equal((await c.estado()).somenteLeitura, false, '175 s: não volta a bloquear')
    await c.enter()
    assert.deepEqual((await c.estado()).enviados, [RASCUNHO], 'o rascunho preservado foi enviado')
  } finally { await fecha(c) }
})

test('desmontar e remontar o Composer no meio da compactação não reinicia os 90 s', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.compactando() // t = 0
    assert.equal((await c.estado()).somenteLeitura, true)
    await c.correr(60_000)
    await c.page.evaluate(() => { (window as any).__raiz.unmount() }) // sai da conversa
    await c.assentar()
    assert.equal(await c.page.evaluate(() => (window as any).__modelo.compactacao('b', null)), null, 'desmontar solta o envio e o relógio')
    await c.page.evaluate(() => { (window as any).__montar() }) // volta à conversa; o Hub ainda compacta
    await c.assentar()
    assert.equal((await c.estado()).somenteLeitura, true, 'remontou aos 60 s: ainda espera')
    await c.correr(29_999)
    assert.equal((await c.estado()).somenteLeitura, true)
    await c.correr(1)
    assert.equal((await c.estado()).somenteLeitura, false, 'o prazo seguiu contando do primeiro sinal')
    assert.equal((await c.estado()).rascunho, RASCUNHO)
  } finally { await fecha(c) }
})

test('colagem em quarentena + compactação: "Não é senha" espera; resolve depois (fim antes dos 90 s) com o rascunho intacto, sem enviar sozinho', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.page.evaluate((t) => { (window as any).__store.composerDrafts.edit('b', t, 'typed') }, 'a senha é ')
    assert.equal(await c.colar(CHAVE), true, 'colar senha é interceptado pelo app')
    const antes = await c.estado()
    assert.equal(antes.quarentena, true, 'o trecho foi para a quarentena')
    assert.equal(antes.rascunho, 'a senha é ', 'e não entrou no rascunho')
    await c.compactando()
    await c.clicarNaoESenha()
    const durante = await c.estado()
    assert.equal(durante.rascunho, 'a senha é ', 'durante a compactação o rascunho não muda')
    assert.equal(durante.quarentena, true, 'a quarentena segue aberta, esperando')
    await c.correr(30_000)
    await c.terminou()
    const depois = await c.estado()
    assert.equal(depois.somenteLeitura, false)
    assert.equal(depois.rascunho, 'a senha é ', 'terminar não mexe no rascunho')
    assert.deepEqual(depois.enviados, [], 'terminar não envia nada sozinho')
    await c.clicarNaoESenha()
    assert.equal((await c.estado()).rascunho, `a senha é ${CHAVE}`, 'liberado, "Não é senha" resolve')
  } finally { await fecha(c) }
})

test('colagem em quarentena + atividade tardia + 90 s: resolve só depois do prazo do primeiro sinal', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.page.evaluate((t) => { (window as any).__store.composerDrafts.edit('b', t, 'typed') }, 'a senha é ')
    await c.colar(CHAVE)
    await c.compactando() // t = 0
    await c.correr(60_000)
    await c.trocarConexao() // troca no meio
    await c.clicarNaoESenha()
    assert.equal((await c.estado()).rascunho, 'a senha é ')
    await c.correr(29_999)
    await c.clicarNaoESenha()
    assert.equal((await c.estado()).rascunho, 'a senha é ', '89,999 s: ainda congelado')
    await c.correr(1)
    await c.clicarNaoESenha()
    assert.equal((await c.estado()).rascunho, `a senha é ${CHAVE}`, '90 s: resolve')
  } finally { await fecha(c) }
})

test('compactando, nenhuma entrada altera o rascunho: digitar, colar, comando /fast, Enter; ao terminar, tudo volta', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.compactando()
    await c.page.locator('textarea').focus()
    await c.page.keyboard.type('xyz')
    assert.equal((await c.estado()).rascunho, RASCUNHO, 'digitar')
    assert.equal(await c.colar('texto colado'), true, 'colar é barrado')
    assert.equal((await c.estado()).rascunho, RASCUNHO, 'colar')
    await c.enter()
    assert.deepEqual((await c.estado()).enviados, [], 'Enter não envia')
    await c.page.evaluate(() => { (window as any).__store.composerDrafts.edit('b', '/fast', 'typed') }) // o rascunho já era um comando
    await c.enter()
    const cmd = await c.estado()
    assert.equal(cmd.rascunho, '/fast', 'o comando não limpa o campo')
    assert.equal(cmd.fast, 0, 'nem roda')
    // "Experimentar" de uma rotina também não escreve; o texto espera
    await c.page.evaluate((t) => { (window as any).__store.composerDrafts.edit('b', t, 'typed'); (window as any).__store.setRoutineDraft('b', 'texto da rotina') }, RASCUNHO)
    await c.assentar()
    assert.equal((await c.estado()).rascunho, RASCUNHO, 'rotina')
    await c.terminou()
    assert.equal((await c.estado()).rascunho, `${RASCUNHO}\ntexto da rotina`, 'terminou: o texto da rotina que esperava entra (o rascunho não se perdeu)')
    await c.page.evaluate((t) => { (window as any).__store.composerDrafts.edit('b', t, 'typed') }, RASCUNHO)
    await c.enter()
    assert.deepEqual((await c.estado()).enviados, [RASCUNHO], 'terminou: o envio voltou')
  } finally { await fecha(c) }
})

test('trocar de conexão / remontar ANTES de o Hub mostrar a atividade não apaga o primeiro sinal (t=0 pedido, t=60 remonta, t=85 atividade, t=90 libera)', { skip: pular }, async () => {
  const c = await cena()
  try {
    await c.comprimirAgora() // t = 0
    assert.equal((await c.estado()).somenteLeitura, true)
    await c.correr(60_000)
    await c.page.evaluate(() => { (window as any).__raiz.unmount() })
    await c.assentar()
    await c.page.evaluate(() => { (window as any).__montar() }) // volta à conversa aos 60 s, sem atividade ainda
    await c.assentar()
    await c.correr(25_000)
    await c.compactando() // t = 85
    assert.equal((await c.estado()).somenteLeitura, true, '85 s: ainda esperando')
    await c.correr(4_999)
    assert.equal((await c.estado()).somenteLeitura, true)
    await c.correr(1)
    assert.equal((await c.estado()).somenteLeitura, false, '90 s do pedido, não da atividade')
  } finally { await fecha(c) }
})
