import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import test from 'node:test'
import { build } from 'esbuild'
import { chromium, type Browser } from 'playwright-core'

// "O clique ficou travado na torre": as telas que cobrem a janela pausam as assinaturas da árvore coberta, e a faixa
// mora nela. Este teste monta a SpaceRail real, com React DOM num Chromium, dentro de uma camada que pausa como as
// do app (rotinas-central/camada, torre/camada), e troca de lugar com a tela coberta: o item ativo tem de acompanhar.
// Sem Chromium, pula com aviso (CHROME=/caminho).
const noPath = (nome: string) => (process.env.PATH ?? '').split(':').map((d) => `${d}/${nome}`).find((c) => existsSync(c))
const exe = [process.env.CHROME, '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable',
  noPath('chromium'), noPath('chromium-browser'), noPath('google-chrome'), noPath('chrome'), chromium.executablePath()]
  .find((c): c is string => !!c && existsSync(c))
const pular = exe ? false : 'sem Chromium (defina CHROME=/caminho): o teste da faixa foi pulado'

const bundle = pular ? null : await build({
  stdin: {
    contents: `import { createRoot } from 'react-dom/client';
      import { createElement as h } from 'react';
      import { SubscriptionPaused, useModel } from './lib/observable';
      import { SpaceRail } from './views/shell/SpaceRail';
      import { FaixaViva } from './views/fork/faixa/FaixaViva';
      import { torreScreen } from './views/fork/torre/lugar-atual';
      import { routinesScreen } from './views/rotinas-central/navigation';
      import { extensoesScreen } from './views/fork/extensoes/lugar-atual';
      const noop = () => {};
      function Camada({ children }) {
        const t = useModel(torreScreen).open, r = useModel(routinesScreen).open, e = useModel(extensoesScreen).open;
        const coberta = t || r || e;
        return h(SubscriptionPaused.Provider, { value: coberta }, children);
      }
      const rail = h(SpaceRail, { onAccount: noop, accountOpen: false, onSkills: noop, onConnectors: noop, onUsage: noop, onSettings: noop,
        approvals: 0, onApprovals: noop, extensionsAvailable: true });
      window.App = {
        montar: () => createRoot(document.getElementById('root')).render(h(Camada, null, h(FaixaViva, null, rail))),
        torre: () => torreScreen.show(null, 'c'),
        rotinas: () => routinesScreen.show(),
        conectores: () => extensoesScreen.show('connectors', 'c'),
      };`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'browser', format: 'iife', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
  logLevel: 'silent',
})

const INICIO = `
  window.__importMeta = { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } };
  const sem = () => new Proxy(function () {}, { get: (_, k) => (k === 'then' ? (ok) => ok?.() : sem()), apply: () => sem() });
  window.codync = new Proxy({ platform: 'linux' }, { get: (t, k) => (k in t ? t[k] : sem()) });
  localStorage.setItem('appLanguage', '"pt-BR"');
`

let browser: Browser
test.before(async () => { if (!pular) browser = await chromium.launch({ executablePath: exe, args: ['--no-sandbox'] }) })
test.after(async () => { await browser?.close() })

test('a faixa acompanha a troca de lugar com a janela coberta (a torre não fica marcada)', { skip: pular }, async () => {
  const page = await browser.newPage()
  const erros: string[] = []
  page.on('pageerror', (e) => erros.push(e.message))
  await page.addInitScript(INICIO)
  await page.route('https://app.test/**', (r) => r.fulfill({ contentType: 'text/html', body: '<!doctype html><html lang="pt-BR"><body><div id="root"></div></body></html>' }))
  await page.goto('https://app.test/')
  await page.addScriptTag({ content: bundle!.outputFiles[0]!.text })
  const atual = async () => {
    await page.waitForTimeout(50)
    return page.evaluate(() => [...document.querySelectorAll('.space-rail [aria-current="page"]')].map((b) => b.className.includes('torre-open') ? 'torre' : b.className.includes('rc-open') ? 'rotinas' : b.getAttribute('aria-label')))
  }
  await page.evaluate(() => (window as any).App.montar())
  assert.deepEqual(await atual(), [])
  await page.evaluate(() => (window as any).App.torre())
  assert.deepEqual(await atual(), ['torre'])
  await page.evaluate(() => (window as any).App.rotinas())
  assert.deepEqual(await atual(), ['rotinas'])
  await page.evaluate(() => (window as any).App.torre())
  await page.evaluate(() => (window as any).App.conectores())
  assert.deepEqual(await atual(), ['Conectores'])
  assert.deepEqual(erros, [])
})
