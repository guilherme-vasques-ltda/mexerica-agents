import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import test from 'node:test'
import { build } from 'esbuild'
import { chromium } from 'playwright-core'

const exe = [process.env.CHROME, '/usr/bin/chromium', '/usr/bin/google-chrome', chromium.executablePath()]
  .find((path): path is string => !!path && existsSync(path))

test('o pulo sobrevive à conversa e a mesma conta recupera a leitura após confirmação', { skip: !exe && 'sem Chromium' }, async () => {
  const bundle = await build({
    stdin: {
      contents: `
        import { createElement, useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import { flushSync } from 'react-dom';
        import { useOnboardingSession } from './views/fork/onboarding/useOnboardingSession';
        import { account } from './store/account';
        import { Observable } from './lib/observable';
        const local = Object.assign(new Observable(), { computer: { id: 'device' }, hello: { hostId: 'hub' }, principal: Object.assign(new Observable(), { id: 'berga' }), bots: new Map(), connection: { kind: 'offline' } });
        const app = { local, computers: [], spaceMirrors: { active: null }, selection: null };
        account.isReady = true;
        account.user = { userId: 'alice' };
        function Tour({ controller }) {
          return createElement('button', { id: 'tour', onClick: controller.dismissFirstChat }, String(controller.firstChatDismissed));
        }
        function App() {
          const [mount, update] = useState(0);
          const session = useOnboardingSession(app, { kind: 'notInstalled' }, true, null);
          window.remountTour = () => flushSync(() => update(x => x + 1));
          window.changeHub = (hostId) => { local.hello = { hostId }; window.remountTour(); };
          window.changeAccount = (id) => { account.user = id ? { userId: id } : null; account.sessionRevision++; window.remountTour(); };
          return createElement('div', { 'data-show': String(session.showStep3) }, createElement(Tour, { key: mount, controller: session.controller }));
        }
        const root = createRoot(document.getElementById('root'));
        flushSync(() => root.render(createElement(App)));
      `,
      resolveDir: new URL('../../../', import.meta.url).pathname,
      loader: 'ts',
    },
    bundle: true, write: false, platform: 'browser', jsx: 'automatic',
    alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
    define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__testMeta', '__DONNA_DEV_CLOUD_ACCESS__': 'false', '__MEXERICA_OFFICIAL_DESKTOP__': 'false' },
    loader: { '.css': 'empty', '.png': 'empty', '.svg': 'empty', '.jpg': 'empty', '.webp': 'empty', '.mp4': 'empty' },
    logLevel: 'silent',
  })
  const browser = await chromium.launch({ executablePath: exe, args: ['--no-sandbox'] })
  try {
    const page = await browser.newPage()
    await page.route('https://app.example.invalid/**', route => route.fulfill({ contentType: 'text/html', body: '<div id="root"></div>' }))
    await page.goto('https://app.example.invalid/')
    await page.evaluate(`
      window.__testMeta = { glob: () => ({}), env: {} };
      window.calls = [];
      window.accountListeners = new Set();
      window.confirmed = false;
      const empty = () => () => {};
      window.codync = new Proxy({ platform: 'linux',
        account: { onChange: fn => { accountListeners.add(fn); return () => accountListeners.delete(fn); } },
        host: { onboardingDismissal: async (user, action) => { calls.push([user, action]); return confirmed ? user === 'bob' : null; } },
      }, { get: (obj, key) => obj[key] ?? new Proxy({}, { get: () => empty }) });
      window.revalidate = (user) => { for (const listener of accountListeners) listener({ ready: true, busy: false, user: { userId: user } }); };
    `)
    await page.addScriptTag({ content: bundle.outputFiles[0]!.text })
    await page.waitForFunction('calls.length === 1')
    assert.equal(await page.locator('[data-show]').getAttribute('data-show'), 'false')
    await page.evaluate('revalidate("bob")')
    assert.equal(await page.evaluate('calls.length'), 1)
    await page.evaluate('confirmed = true; revalidate("alice")')
    await page.waitForSelector('[data-show="true"]')
    assert.deepEqual(await page.evaluate('calls'), [['alice', 'read'], ['alice', 'read']])

    await page.locator('#tour').click()
    await page.waitForFunction('document.querySelector("#tour").textContent === "true"')
    await page.evaluate('remountTour()')
    assert.equal(await page.locator('#tour').textContent(), 'true')
    await page.evaluate('changeHub("other-hub")')
    assert.equal(await page.locator('#tour').textContent(), 'false')
    await page.evaluate('changeHub("hub")')
    assert.equal(await page.locator('#tour').textContent(), 'false')

    await page.evaluate('changeAccount("bob")')
    await page.waitForFunction('calls.some(([user]) => user === "bob")')
    assert.equal(await page.locator('[data-show]').getAttribute('data-show'), 'false')
    assert.equal(await page.locator('#tour').textContent(), 'false')
    await page.evaluate('changeAccount(null)')
    const reads = await page.evaluate('calls.length')
    await page.evaluate('revalidate("bob")')
    assert.equal(await page.evaluate('calls.length'), reads)
  } finally {
    await browser.close()
  }
})
