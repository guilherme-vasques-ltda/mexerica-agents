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
export function DropdownMenu({ children }) { return children }
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
  [new URL('../../../components/Text.tsx', import.meta.url).href, `export function Text({ children }) { return children }`],
  [new URL('../../../store/context.ts', import.meta.url).href, `export function useApp() { return globalThis.fixtureApp }`],
  [new URL('../../settings/ssh-model.ts', import.meta.url).href, `export function useSSH() { return { state: { profiles: [], attachments: [] } } }`],
  [new URL('../../settings/parts.tsx', import.meta.url).href, `export function rotuloConexao() { return '' }`],
])

registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('@shared/')) {
      return next(new URL(`${specifier.slice('@shared/'.length)}.ts`, sharedRoot).href, context)
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

test('Instalação direta informa a recusa no Desktop Linux e preserva as outras ações', async () => {
  const { forkFeatures } = await import('../../../../shared/fork-features.ts')
  const { HostStateView } = await import('../../HostStateView.tsx')
  const { useComputerMenu } = await import('../../ComputerFilterHeader.tsx')
  const { Step3Wizard } = await import('./Step3Wizard.tsx')
  const aviso = 'A instalação assistida do Hub não está disponível no aplicativo para Linux.'
  const hostState = { kind: 'notInstalled' } as const
  const host = { state: hostState, install: () => { installs += 1 } }
  let installs = 0
  const noop = () => {}
  const desktop = { codync: { platform: 'linux', host } }
  const originalWindow = globalThis.window
  const localHost = forkFeatures.localHost
  forkFeatures.localHost = true
  Object.assign(globalThis, { fixtureApp: { host, computers: [] } })
  const menu = () => {
    let items: ReturnType<ReturnType<typeof useComputerMenu>['items']> = []
    function Fixture() {
      items = useComputerMenu('', noop, noop).items()
      return null
    }
    renderToStaticMarkup(createElement(Fixture))
    return items
  }
  const wizard = (platform: 'linux' | 'darwin' | 'win32', hostState: Parameters<typeof Step3Wizard>[0]['hostState']) => renderToStaticMarkup(createElement(Step3Wizard, {
    client: null, spaceId: 'pessoal', etapaInicial: 'onde', hostState, localHubSupported: true, platform, status: null, where: 'local',
    onSelectWhere: noop, onOpenComputers: noop, onSkip: noop, onContinue: noop,
  }))
  try {
    Object.assign(globalThis, { window: { ...originalWindow, ...desktop } })
    const row = menu().find((item) => item.title === aviso)
    assert.ok(row?.disabled)
    row.action()
    assert.equal(installs, 0)
    const hostHtml = renderToStaticMarkup(createElement(HostStateView))
    assert.ok(hostHtml.includes(aviso))
    assert.doesNotMatch(hostHtml, /<button/)
    const linux = wizard('linux', hostState)
    assert.ok(linux.includes(aviso))
    assert.doesNotMatch(linux, /onboarding-consent-box/)

    for (const platform of ['darwin', 'win32', 'linux'] as const) {
      Object.assign(globalThis, { window: { ...originalWindow, codync: { platform, host, web: platform === 'linux' } } })
      const install = menu().find((item) => !item.disabled && /instalar/i.test(item.title))
      assert.ok(install)
      install.action()
      assert.match(renderToStaticMarkup(createElement(HostStateView)), /<button/)
      assert.match(wizard(platform, hostState), /onboarding-consent-box/)
      assert.doesNotMatch(wizard(platform, { kind: 'disabled' }), /onboarding-consent-box/)
    }
    assert.equal(installs, 3)
  } finally {
    forkFeatures.localHost = localHost
    Object.assign(globalThis, { window: originalWindow, fixtureApp: undefined })
  }
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
        spaceId: 'pessoal',
      etapaInicial: 'onde',
      hostState: { kind: 'disabled' },
      platform: 'darwin',
      status: macStatus,
      where: 'local',
      onSelectWhere() {},
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
      spaceId: 'pessoal',
      etapaInicial: 'onde',
      hostState: { kind: 'running' },
      platform: 'linux',
      status: {
        ...macStatus,
        hostOs: 'linux',
        spaceContext: { currentSpaceId: 'pessoal', personalSpaceId: 'pessoal' },
        capabilities: { ...macStatus.capabilities, motorMexerica: true, localKeysAndOAuth: true },
      },
      where: 'remote',
      onSelectWhere() {},
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

test('Entrada guiada pergunta 100% local ou com conta; sem login na versão, "Com conta" e "nossa nuvem" ficam em breve', async () => {
  const { Step3Wizard } = await import('./Step3Wizard.tsx')
  const tela = (etapaInicial: 'modo' | 'nuvem') => renderToStaticMarkup(createElement(Step3Wizard, {
    client: null, spaceId: 'pessoal', etapaInicial, hostState: { kind: 'running' }, platform: 'linux', status: null, where: 'local',
    onSelectWhere() {}, onOpenComputers() {}, onSkip() {}, onContinue() {},
  }))
  const modo = tela('modo')
  assert.match(modo, /Como você quer usar a Mexerica\?/)
  assert.match(modo, /Passo 2 de 5/)
  assert.match(modo, /<button[^>]*role="radio" aria-checked="true"[^>]*>(?:(?!<\/button>).)*100% local/)
  assert.match(modo, /<button[^>]*role="radio" aria-checked="false" disabled=""[^>]*>(?:(?!<\/button>).)*Com conta(?:(?!<\/button>).)*Em breve/)
  assert.match(modo, /Nada sai deste computador, a não ser o que vai para o provedor de IA que você conectar/)
  assert.match(modo, /Sem conta e sem telemetria/)

  const nuvem = tela('nuvem')
  assert.match(nuvem, /Passo 3 de 6/)
  assert.match(nuvem, /<button[^>]*role="radio" aria-checked="true"[^>]*>(?:(?!<\/button>).)*Na sua infraestrutura/)
  assert.match(nuvem, /<button[^>]*role="radio" aria-checked="false" disabled=""[^>]*>(?:(?!<\/button>).)*Na nossa nuvem(?:(?!<\/button>).)*Em breve/)
})
