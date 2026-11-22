import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'

const sourceRoot = new URL('../../../', import.meta.url).href
const sharedRoot = new URL('../../../../shared/', import.meta.url).href

const stubs = new Map([
  [
    new URL('../../../components/Controls.tsx', import.meta.url).href,
    `import { createElement } from 'react'
export function Spinner() { return createElement('span', { 'data-spinner': '' }) }
export function Button({ children, disabled, type }) { return createElement('button', { type: type ?? 'button', disabled }, children) }`,
  ],
  [
    new URL('../../../components/Icon.tsx', import.meta.url).href,
    `import { createElement } from 'react'
export function Icon({ name }) { return createElement('span', { 'data-icon': name }) }`,
  ],
  [
    new URL('../../../components/Avatar.tsx', import.meta.url).href,
    `import { createElement } from 'react'
export function BotAvatar({ bot }) { return createElement('span', { 'data-avatar': bot?.name ?? '' }) }`,
  ],
])

registerHooks({
  resolve(specifier, context, next) {
    if (specifier === '@shared/fork-features') {
      return next(new URL('../../../../shared/fork-features.ts', import.meta.url).href, context)
    }
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      for (const ext of ['.ts', '.tsx']) {
        const candidate = new URL(`${specifier}${ext}`, context.parentURL)
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
    if (/\.(css|svg|png)$/.test(clean.pathname)) {
      return { format: 'module', shortCircuit: true, source: 'export default "fixture-asset"' }
    }
    if ((clean.href.startsWith(sourceRoot) || clean.href.startsWith(sharedRoot)) && /\.tsx?$/.test(clean.pathname)) {
      let source = ts.transpileModule(readFileSync(clean, 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
      }).outputText
      if (clean.href.endsWith('/fork-features.ts')) {
        source = source.replaceAll('__DONNA_DEV_CLOUD_ACCESS__', 'false')
      }
      return { format: 'module', shortCircuit: true, source }
    }
    return next(url, context)
  },
})

Object.assign(globalThis, {
  localStorage: {
    getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null),
    setItem() {},
    removeItem() {},
  },
  document: { documentElement: { lang: 'pt-BR' }, body: { setAttribute() {} }, hidden: false, hasFocus: () => true },
  window: { codync: { platform: 'linux' }, addEventListener() {} },
})

test('Step3Wizard exige consentimento local, mostra bloqueio honesto e respeita capacidade do SO', async () => {
  const { Step3Wizard } = await import('./Step3Wizard.tsx')
  const macStatus = {
    hubReady: true,
    hostOs: 'darwin',
    spaceContext: { currentSpaceId: 'pessoal', personalSpaceId: 'pessoal' },
    capabilities: {
      motorMexerica: false,
      localKeysAndOAuth: false,
      engineAccounts: true,
      computerControl: true,
      voice: true,
    },
    ai: { hasReadyAi: false, connectedAccounts: 0, configuredKeys: 0, providers: [] },
    voice: { configured: false },
    computerAccess: { available: true, enabled: false, connected: false, capture: false, input: false, ready: false },
    readiness: {
      readyCount: 1,
      totalCount: 4,
      items: [
        { id: 'hub' as const, ready: true, available: true },
        { id: 'ai' as const, ready: false, available: true },
        { id: 'voice' as const, ready: false, available: true },
        { id: 'computer' as const, ready: false, available: true },
      ],
    },
    persisted: null,
  }

  const disabledMacHtml = renderToStaticMarkup(
      createElement(Step3Wizard, {
        client: null,
        accountKey: 'user-fixture',
        spaceId: 'pessoal',
      hostState: { kind: 'disabled' },
      platform: 'darwin',
      status: macStatus,
      initialWhere: 'local',
      onSelectWhere() {},
      onStatusChange() {},
      onOpenComputers() {},
      onSkip() {},
      onContinue() {},
    }),
  )
  assert.match(disabledMacHtml, /instaladores oficiais desta versão/)
  assert.doesNotMatch(disabledMacHtml, /Serviço local pronto/)
  assert.doesNotMatch(disabledMacHtml, /Entrar com ChatGPT/)

  const remoteConfirmHtml = renderToStaticMarkup(
    createElement(Step3Wizard, {
      client: {} as never,
      accountKey: 'user-fixture',
      spaceId: 'pessoal',
      hostState: { kind: 'running' },
      platform: 'linux',
      status: {
        ...macStatus,
        hostOs: 'linux',
        spaceContext: { currentSpaceId: 'pessoal', personalSpaceId: 'pessoal' },
        capabilities: { ...macStatus.capabilities, motorMexerica: true, localKeysAndOAuth: true },
      },
      initialWhere: 'remote',
      onSelectWhere() {},
      onStatusChange() {},
      onOpenComputers() {},
      onSkip() {},
      onContinue() {},
    }),
  )
  assert.match(remoteConfirmHtml, /instalação guiada em servidor novo ainda aguarda o contrato oficial/i)
  assert.doesNotMatch(remoteConfirmHtml, /Confiar neste computador/)
  assert.doesNotMatch(remoteConfirmHtml, /Entrar com ChatGPT/)
  assert.doesNotMatch(remoteConfirmHtml, /Detectar o que já existe neste computador/)
})

test('FirstChatOnboarding, ComposerSpotlight e ReadinessCard renderizam 6 tarefas, spotlight e N de M real', async () => {
  const { FirstChatOnboarding } = await import('./FirstChatOnboarding.tsx')
  const { ComposerSpotlight } = await import('./ComposerSpotlight.tsx')
  const { ReadinessCard } = await import('./ReadinessCard.tsx')

  const firstChatHtml = renderToStaticMarkup(
    createElement(FirstChatOnboarding, {
      bot: { id: 'berga', name: 'Berga', backend: 'codex', cwd: '/tmp', status: 'idle', unread: 0 } as never,
      hasReadyAi: false,
      onPickTask() {},
      onConnectAi() {},
    }),
  )
  assert.match(firstChatHtml, /Organizar minha semana/)
  assert.match(firstChatHtml, /Pesquisar e resumir/)
  assert.match(firstChatHtml, /E-mails e posts/)
  assert.match(firstChatHtml, /Responder clientes/)
  assert.match(firstChatHtml, /Planilhas e números/)
  assert.match(firstChatHtml, /Programar e automatizar/)
  assert.match(firstChatHtml, /Conectar IA agora/)

  const spotlightHtml = renderToStaticMarkup(
    createElement(ComposerSpotlight, { open: true, onDismiss() {}, onSkipTour() {} }),
  )
  assert.match(spotlightHtml, /ÚLTIMO PASSO/)
  assert.match(spotlightHtml, /Entendi/)

  const readinessHtml = renderToStaticMarkup(
    createElement(ReadinessCard, {
      status: {
        hubReady: true,
        hostOs: 'linux',
        capabilities: { motorMexerica: true, localKeysAndOAuth: true, engineAccounts: true, computerControl: true, voice: true },
        ai: { hasReadyAi: true, connectedAccounts: 1, configuredKeys: 0, providers: [] },
        voice: { configured: false },
        computerAccess: { available: true, enabled: false, connected: false, capture: false, input: false, ready: false },
        readiness: {
          readyCount: 2,
          totalCount: 4,
          items: [
            { id: 'hub', ready: true, available: true },
            { id: 'ai', ready: true, available: true },
            { id: 'voice', ready: false, available: true },
            { id: 'computer', ready: false, available: true },
          ],
        },
        persisted: null,
      },
      onConfigurePermissions() {},
    }),
  )
  assert.match(readinessHtml, /Pronta para trabalhar · 2 de 4/)
  assert.match(readinessHtml, /Configurar permissões/)
})
