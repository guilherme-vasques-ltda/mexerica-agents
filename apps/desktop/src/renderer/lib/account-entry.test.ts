import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'
import type { WindowCommand } from '../../shared/ipc.ts'

const features = new URL('../../shared/fork-features.ts', import.meta.url)
const sourceRoot = new URL('../../', import.meta.url).href
const hooksApagar = new URL('./fixture-hooks-apagar', import.meta.url).href
// Each case sets the account projection the gate reads (globalThis.__accountEntryFixture).
const fixtureAccount = 'export function useAccount() { return globalThis.__accountEntryFixture }'
// Only the account snapshot and neighboring screens are replaced. The gate ChatWindow uses, the
// welcome entrypoint, WelcomeActions, the sign-in methods, the recovery panel with its copy and the
// build-only feature selector are the real sources.
const stubs = new Map([
  [new URL('../store/account.ts', import.meta.url).href, fixtureAccount],
  [new URL('../components/Controls.tsx', import.meta.url).href, `import { createElement } from 'react'
export function Spinner() { return null }
export function Button({ children, disabled }) { return createElement('button', { type: 'button', disabled }, children) }`],
  [new URL('../components/Icon.tsx', import.meta.url).href, 'export function Icon() { return null }'],
  [new URL('../components/Overlay.tsx', import.meta.url).href, `import { createElement } from 'react'
export function Dialog({ open, message }) { return open ? createElement('div', { role: 'alertdialog' }, message) : null }`],
  // DON-903: o passo de idioma (tema, catálogos) fica fora; ele só aparece antes da conta e não decide o portão.
  [new URL('../views/fork/idioma/PassoIdioma.tsx', import.meta.url).href, 'export function PassoIdioma() { return null }\nexport function usePassoIdioma() { return { pendente: false, concluido: false, concluir() {} } }'],
])

registerHooks({
  resolve(specifier, context, next) {
    if (specifier === 'react' && context.parentURL?.endsWith('AccountEntryGate.tsx?accountEntry=disabled&provaApagar=1')) {
      return { url: hooksApagar, shortCircuit: true }
    }
    if (specifier === '@shared/fork-features') {
      const variant = new URL(context.parentURL!).searchParams.get('accountEntry')
      return next(`${features.href}?accountEntry=${variant ?? 'disabled'}`, context)
    }
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      for (const extension of ['.ts', '.tsx']) {
        const candidate = new URL(`${specifier}${extension}`, context.parentURL)
        if (existsSync(candidate)) return next(candidate.href, context)
      }
    }
    return next(specifier, context)
  },
  load(url, context, next) {
    if (url === hooksApagar) return { format: 'module', shortCircuit: true, source: `
export function useEffect(effect) { effect() }
export function useState() {
  return [globalThis.__falhouAoApagarNaFixture, valor => { globalThis.__falhouAoApagarNaFixture = valor }]
}` }
    if (!url.startsWith('file:')) return next(url, context)
    const clean = new URL(url)
    clean.search = ''
    const stub = stubs.get(clean.href)
    if (stub !== undefined) return { format: 'module', shortCircuit: true, source: stub }
    if (/\.(css|svg|png)$/.test(clean.pathname)) {
      return { format: 'module', shortCircuit: true, source: 'export default "fixture-asset"' }
    }
    if (clean.href.startsWith(sourceRoot) && /\.tsx?$/.test(clean.pathname)) {
      let source = ts.transpileModule(readFileSync(clean, 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
      }).outputText
      if (clean.href === features.href) {
        // Compiler fixture only: never a runtime preference or authority proof.
        const value = new URL(url).searchParams.get('accountEntry') === 'enabled' ? 'true' : 'false'
        source = source.replaceAll('__DONNA_DEV_CLOUD_ACCESS__', value)
      }
      return { format: 'module', shortCircuit: true, source }
    }
    return next(url, context)
  },
})

/** The main process's projection as the renderer store exposes it; `canAct` follows the store's rule. */
function projection(overrides: Partial<{ socialProviders: string[]; isReady: boolean; isConfigured: boolean; isSignedIn: boolean; isBusy: boolean; isSigningOut: boolean; exit: string; isExitUnresolved: boolean; exitChoices: string[] }> = {}) {
  const state = {
    isReady: true, isConfigured: true, isSignedIn: false, isBusy: false, isSigningOut: false, exit: 'none', isExitUnresolved: false,
    exitChoices: [] as string[], exitAction: null, exitProvider: null, exitOutcome: null, exitRegistrationRequired: false,
    restrictedCleanupRequired: false, hasOldConfirmations: false, confirmationCount: null, confirmationCursor: null, confirmationOutcome: null,
    // Dev instance: Apple, Google and X on (Main reads it from the Clerk environment).
    socialProviders: ['apple', 'google', 'x'], errorMessage: null, user: null, ...overrides,
  }
  const refuse = (what: string) => () => { throw new Error(`SSR must not ${what}`) }
  return {
    ...state,
    canAct: state.isReady && !state.isBusy && !state.isSigningOut,
    signIn: refuse('start sign-in'),
    signInWithPassword: refuse('send a password'),
    confirmExitAccount: refuse('confirm the account'),
    confirmExitAccountWithPassword: refuse('send a password'),
    checkExit: refuse('check the exit'),
    signOut: refuse('sign out'),
  }
}

const tags = (html: string, tag: string) => [...html.matchAll(new RegExp(`<${tag}\\b([^>]*)>(?:((?:(?!<\\/${tag}>)[\\s\\S])*)<\\/${tag}>)?`, 'g'))]
  .map(match => ({
    attributes: match[1]!,
    // What a person reads: decorative marks (aria-hidden) and markup left out.
    text: (match[2] ?? '').replace(/<span\b[^>]*aria-hidden="true"[^>]*>[\s\S]*?<\/span>/g, '').replace(/<[^>]*>/g, '').trim(),
  }))
// DON-903: o idioma real (lib/fork-i18n/estado) grava lang/dir no documento e ouve o storage da janela.
// Fatia 1 (D454): a conta segue o idioma escolhido; o texto conferido aqui é o pt-BR (escolha salva pt-BR).
const idiomaDocumento = { documentElement: { lang: 'pt-BR' }, body: { setAttribute() {} }, hidden: false, hasFocus: () => true }
Object.assign(globalThis, { localStorage: { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} } })
const isDisabled = (attributes: string) => /\bdisabled(?:\s|=|$)/.test(attributes)
const attribute = (attributes: string, name: string) => new RegExp(`\\b${name}="([^"]*)"`, 'i').exec(attributes)?.[1] ?? null

test('with sign-in in the build, the app mounts only behind a confirmed account; without it, the app opens directly', async () => {
  Object.assign(globalThis, { window: { codync: { platform: 'darwin' }, addEventListener() {} }, document: idiomaDocumento })
  let shellMounts = 0
  // Stands in for ChatShell: rendering it at all means the app's hooks and listeners would start.
  const Shell = () => {
    shellMounts++
    return createElement('main', { 'data-app-shell': '' })
  }
  const render = async (variant: 'enabled' | 'disabled', account: ReturnType<typeof projection>) => {
    Object.assign(globalThis, { __accountEntryFixture: account })
    const url = new URL(`../views/AccountEntryGate.tsx?accountEntry=${variant}`, import.meta.url)
    const { AccountEntryGate } = await import(url.href)
    return renderToStaticMarkup(createElement(AccountEntryGate, null, createElement(Shell)))
  }
  const socialNames = ['Entrar com Apple', 'Entrar com X', 'Entrar com Google']
  const social = (html: string) => tags(html, 'button').filter(button => socialNames.includes(attribute(button.attributes, 'aria-label') ?? ''))
  const form = (html: string, label: string) => tags(html, 'form').find(found => attribute(found.attributes, 'aria-label') === label)
  const locked = (html: string, state: string) => {
    assert.doesNotMatch(html, /data-app-shell/, `${state}: the app must not mount`)
    assert.equal(shellMounts, 0, `${state}: the app must not render`)
    assert.doesNotMatch(html, /Conhecer a Donna|Continuar neste/, `${state}: no way in without the account`)
    return html
  }
  const unavailable = /O login não está disponível nesta versão/

  // Signed out: e-mail and password, then Apple, X and Google, all available and nothing sent yet.
  const anonymous = locked(await render('enabled', projection()), 'signed out')
  assert.doesNotMatch(anonymous, /intro-scene|intro-skip|Skip introduction|Pular apresentação/, 'D505: a entrada abre direto, sem a apresentação')
  assert.ok(form(anonymous, 'Entrar com e-mail e senha'), 'the password sign-in must be reachable in the admitted Dev build')
  const inputs = tags(anonymous, 'input')
  assert.equal(inputs.length, 2)
  const [email, secret] = inputs
  assert.ok(email && secret)
  assert.equal(attribute(email.attributes, 'type'), 'email')
  assert.equal(attribute(email.attributes, 'autocomplete'), 'username')
  assert.equal(attribute(secret.attributes, 'type'), 'password')
  assert.equal(attribute(secret.attributes, 'autocomplete'), 'current-password')
  for (const input of [email, secret]) {
    assert.equal(attribute(input.attributes, 'value'), null, 'typed values never become markup')
    assert.equal(isDisabled(input.attributes), false)
  }
  const enter = tags(anonymous, 'button').find(button => button.text === 'Entrar')
  assert.ok(enter && /type="submit"/.test(enter.attributes))
  assert.ok(isDisabled(enter.attributes), 'nothing to send before both fields are typed')
  assert.deepEqual(social(anonymous).map(button => attribute(button.attributes, 'aria-label')), socialNames)
  assert.ok(social(anonymous).every(button => !isDisabled(button.attributes)))

  // Production instance (only Google on): only the Google button. Environment not read yet: only e-mail.
  const production = locked(await render('enabled', projection({ socialProviders: ['google'] })), 'production providers')
  assert.deepEqual(social(production).map(button => attribute(button.attributes, 'aria-label')), ['Entrar com Google'])
  const unread = locked(await render('enabled', projection({ socialProviders: [] })), 'environment unread')
  assert.equal(social(unread).length, 0)
  assert.doesNotMatch(unread, /ou entre com/)
  assert.ok(form(unread, 'Entrar com e-mail e senha'), 'e-mail and password stay without the Clerk environment')

  // Restoring the saved session: only the wait, not even the old "continue on this Mac".
  const restoring = locked(await render('enabled', projection({ isReady: false, isSignedIn: true })), 'restoring')
  // D454/D495: a espera da restauração é traduzida; com pt-BR escolhido (acima), sai em pt-BR.
  assert.match(restoring, /Verificando conta salva/)
  assert.equal(tags(restoring, 'button').length, 0)
  assert.equal(tags(restoring, 'input').length, 0)

  // An uncertain stored session is an unresolved exit: only the main process's check replaces the login.
  const uncertain = locked(await render('enabled', projection({ isSignedIn: true, exit: 'session_unverified', isExitUnresolved: true, exitChoices: ['check'] })), 'session unverified')
  assert.match(uncertain, /class="account-exit is-welcome idioma-superficie" lang="pt-BR" dir="ltr"/)
  assert.deepEqual(tags(uncertain, 'button').map(button => button.text), ['Tentar de novo', 'Entrar novamente'])
  assert.equal(tags(uncertain, 'input').length, 0)

  // The same account asked again: Apple, X, Google and e-mail with password, all in the recovery panel.
  const sameAccount = locked(await render('enabled', projection({ exit: 'needs_authentication', isExitUnresolved: true, exitChoices: ['authenticate', 'check'] })), 'needs authentication')
  assert.deepEqual(tags(sameAccount, 'button').map(button => button.text), ['Tentar de novo', 'Entrar novamente'])
  assert.equal(form(sameAccount, 'Confirmar com e-mail e senha'), undefined)
  assert.equal(form(sameAccount, 'Entrar com e-mail e senha'), undefined, 'confirming the exit is not a sign-in')
  assert.equal(social(sameAccount).length, 0)

  // A new build without sign-in configured is unavailable, not offline.
  const unconfigured = locked(await render('enabled', projection({ isConfigured: false })), 'not configured')
  assert.match(unconfigured, unavailable)
  assert.equal(social(unconfigured).length, 0)
  assert.equal(tags(unconfigured, 'input').length, 0)

  // A user published while sign-in is still running is not a confirmed session yet; nothing can start.
  const signingIn = locked(await render('enabled', projection({ isSignedIn: true, isBusy: true })), 'signing in')
  assert.equal(social(signingIn).length, 3)
  assert.ok(social(signingIn).every(button => isDisabled(button.attributes)))
  assert.ok(tags(signingIn, 'input').every(input => isDisabled(input.attributes)))

  // Dono, 05/11/2026 (opção B): a build sem sistema de login abre direto, mesmo sem sessão nenhuma.
  for (const account of [projection({ isConfigured: false }), projection({ isReady: false }), projection()]) {
    const open = await render('disabled', account)
    assert.match(open, /data-app-shell/)
    assert.doesNotMatch(open, unavailable)
    assert.doesNotMatch(open, /Boas-vindas ao Donnabot|Entrar com|type="password"/)
  }
  assert.equal(shellMounts, 3)
  shellMounts = 0

  // Confirmed by the main process: the app, and nothing of the welcome.
  const confirmed = await render('enabled', projection({ isSignedIn: true }))
  assert.match(confirmed, /data-app-shell/)
  assert.equal(shellMounts, 1)
  assert.doesNotMatch(confirmed, /Boas-vindas ao Donnabot|Entrar com|type="password"/)

  // Leaving puts the gate back: while signing out, then with the exit the main process projects.
  shellMounts = 0
  locked(await render('enabled', projection({ isSignedIn: true, isSigningOut: true })), 'signing out')
  const pending = locked(await render('enabled', projection({ exit: 'pending', isExitUnresolved: true, exitChoices: ['check'] })), 'exit pending')
  assert.deepEqual(tags(pending, 'button').map(button => button.text), ['Tentar de novo', 'Entrar novamente'])
})

test('the build config decides the gate: qualified production main requires sign-in, the empty repo config opens directly', async () => {
  const { admitsCloudBuild } = await import('../../shared/fork-cloud-build.ts')
  const { emptyMainRelease, productionRelease } = await import('../../shared/fork-cloud-build.fixture.ts')
  Object.assign(globalThis, { window: { codync: { platform: 'darwin' }, addEventListener() {} }, document: idiomaDocumento, __accountEntryFixture: projection() })
  // The same literal electron-vite.config.ts defines from resources/account-config.json.
  const gate = async (config: unknown) => {
    const url = new URL(`../views/AccountEntryGate.tsx?accountEntry=${admitsCloudBuild(config) ? 'enabled' : 'disabled'}`, import.meta.url)
    const { AccountEntryGate } = await import(url.href)
    return renderToStaticMarkup(createElement(AccountEntryGate, null, createElement('main', { 'data-app-shell': '' })))
  }
  const production = await gate(productionRelease().input)
  assert.doesNotMatch(production, /data-app-shell/, 'signed out on a qualified production build: the gate stays')
  assert.match(production, /Entrar com e-mail e senha/)
  assert.match(await gate(emptyMainRelease), /data-app-shell/, 'no sign-in configured: the app opens directly')
})

test('a password leaves its field as soon as it is sent; an incomplete form sends and clears nothing', async () => {
  const { takeCredentials } = await import(new URL('../views/welcome/SignInMethods.tsx', import.meta.url).href)
  const identifier = { value: '  pessoa@example.test ' }
  const password = { value: 'fixture-only' }
  assert.deepEqual(takeCredentials(identifier, password), { identifier: 'pessoa@example.test', password: 'fixture-only' })
  assert.equal(password.value, '')
  assert.equal(identifier.value, '  pessoa@example.test ')
  const blank = { value: '   ' }
  const kept = { value: 'fixture-only' }
  assert.equal(takeCredentials(blank, kept), null)
  assert.equal(kept.value, 'fixture-only')
  assert.equal(takeCredentials({ value: 'pessoa@example.test' }, { value: '' }), null)
})

test('a entrada avisa quando o processo principal não consegue apagar os dados', async () => {
  let onCommand: ((comando: WindowCommand) => void) | undefined
  let falhar = false
  Object.assign(globalThis, {
    window: {
      codync: { app: {
        onCommand: (receber: typeof onCommand) => { onCommand = receber; return () => { onCommand = undefined } },
        resetAllData: async () => { if (falhar) throw new Error('detalhe interno da fixture') },
      } },
      addEventListener() {},
    },
    document: idiomaDocumento,
    __accountEntryFixture: projection(),
    __falhouAoApagarNaFixture: false,
  })
  const url = new URL('../views/AccountEntryGate.tsx?accountEntry=disabled&provaApagar=1', import.meta.url)
  const { AccountEntryGate } = await import(url.href)
  const render = () => renderToStaticMarkup(createElement(AccountEntryGate))
  assert.doesNotMatch(render(), /alertdialog/)
  assert.ok(onCommand)
  onCommand({ kind: 'confirmReset' })
  await Promise.resolve()
  assert.doesNotMatch(render(), /alertdialog/)
  falhar = true
  onCommand({ kind: 'confirmReset' })
  await Promise.resolve()
  const falha = render()
  assert.match(falha, /role="alertdialog">Não foi possível apagar todos os dados/)
  assert.doesNotMatch(falha, /detalhe interno/)
})

test('ChatWindow is only the gate around the app; the gate and the sign-in methods never reach preferences, storage or analytics', () => {
  const parse = (path: string) => {
    const file = new URL(path, import.meta.url)
    return ts.createSourceFile(file.pathname, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  }
  const chat = parse('../views/ChatWindow.tsx')
  const declared = (name: string) => chat.statements.find((s): s is ts.FunctionDeclaration => ts.isFunctionDeclaration(s) && s.name?.text === name)
  const chatWindow = declared('ChatWindow')
  const shell = declared('ChatShell')
  assert.ok(chatWindow?.body && shell?.body)
  assert.equal(chatWindow.body.statements.length, 1)
  assert.equal(chatWindow.body.statements[0]!.getText(chat).replace(/\s+/g, ''), 'return(<AccountEntryGate><ChatShell/></AccountEntryGate>)')
  // The app's attachments and window commands moved into the shell, behind the gate.
  const shellText = shell.body.getText(chat)
  for (const call of ['useSSHAttachments(app)', 'useForkCloudAttachments(app)', 'window.codync.app.onCommand(']) assert.ok(shellText.includes(call), call)
  assert.doesNotMatch(chat.getFullText(), /onboardingDone|AccountWelcomeView/)

  const valueImports = (path: string) => parse(path).statements.filter(ts.isImportDeclaration)
    .filter(d => !d.importClause?.isTypeOnly)
    .map(d => (d.moduleSpecifier as ts.StringLiteral).text).sort()
  assert.deepEqual(valueImports('../views/AccountEntryGate.tsx'), ['../components/Overlay', '../lib/fork-i18n/index.ts', '../store/account', './AccountWelcomeView', './fork/account/ConfirmarSairWeb', '@shared/fork-features', 'react'])
  // The password stays in its field: the methods module reads no store, preference, storage or analytics.
  assert.deepEqual(valueImports('../views/welcome/SignInMethods.tsx'), ['../../assets/google.svg', '../../components/Controls', '../../components/Icon', 'react'])
  assert.doesNotMatch(parse('../views/welcome/SignInMethods.tsx').getFullText(), /localStorage|sessionStorage|console\.|useState<string>/)
})
