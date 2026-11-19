import assert from 'node:assert/strict'
import test from 'node:test'
import * as path from 'node:path'
import { deferred, loadMainModule } from '../../shared/test-support/main-module.ts'

type Exit = { code?: number | string; killed?: boolean } | null
type Os = {
  files?: Record<string, string | null>
  status?: () => string | null | undefined
  run?: (bin: string, args: string[]) => Exit | Promise<Exit>
  uid?: number | null
}

const absent = (bin: string): Exit => ({ code: bin === '/bin/launchctl' ? 113 : 1 })
const helper = (bundle: string, name: string) => `${bundle}/Contents/Library/LoginItems/${name}.app/Contents/MacOS/${name}`
const MAIN = '/Applications/Donnabot.app'
const DEV = '/Applications/Donnabot Dev.app'
const HOME_DEV = '/Users/dono/Applications/Donnabot Dev.app'

/** observerSignals real; FS, login item, launchctl e pgrep substituídos. `files`: caminho → código de erro (null = existe). */
function loadObservers(os: Os = {}) {
  const calls: string[] = []
  const module = loadMainModule<typeof import('./observers.ts')>('fork-secret-box/observers', {
    'node:child_process': { execFile: (bin: string, args: string[], _options: unknown, done: (error: Exit) => void) => {
      calls.push(`${bin} ${args.join(' ')}`)
      void Promise.resolve((os.run ?? absent)(bin, args)).then(done)
    } },
    'node:fs': { lstatSync: (file: string) => {
      const code = file in (os.files ?? {}) ? os.files![file] : 'ENOENT'
      if (code === null) return {}
      throw Object.assign(new Error(code), { code })
    } },
    'node:os': { homedir: () => '/Users/dono' },
    'node:path': path,
    '../screen': { screenAgentRawStatus: () => (os.status ?? (() => 'not-registered'))() },
  }, { process: { resourcesPath: `${MAIN}/Contents/Resources`, getuid: os.uid === null ? undefined : () => os.uid ?? 501 } })
  return { module, calls }
}

const signals = async (os: Os) => JSON.parse(JSON.stringify(await loadObservers(os).module.observerSignals())) as string[]

test('as duas variantes sem binário, launchd e processo: nenhum sinal; pgrep antes do launchctl', async () => {
  const f = loadObservers()
  assert.deepEqual(JSON.parse(JSON.stringify(await f.module.observerSignals())), [])
  assert.deepEqual(f.calls, [
    '/usr/bin/pgrep -f -- /DonnabotScreen.app/Contents/MacOS/DonnabotScreen',
    '/usr/bin/pgrep -f -- /DonnabotDevScreen.app/Contents/MacOS/DonnabotDevScreen',
    '/bin/launchctl print gui/501/app.donnabot.screen',
    '/bin/launchctl print gui/501/app.donnabot.dev.screen',
  ])
})

test('R1-01: ajudante da outra variante (Dev ou main) é observador, inclusive no bundle separado', async () => {
  const running = (name: string) => (bin: string, args: string[]) => bin === '/usr/bin/pgrep' && args[2].includes(`/${name}.app/`) ? null : absent(bin)
  assert.deepEqual(await signals({ run: running('DonnabotDevScreen') }), ['DonnabotDevScreen:processo:sim'])
  assert.deepEqual(await signals({ run: running('DonnabotScreen') }), ['DonnabotScreen:processo:sim'])
  const loaded = (bin: string, args: string[]) => bin === '/bin/launchctl' && args[1].endsWith('app.donnabot.dev.screen') ? null : absent(bin)
  assert.deepEqual(await signals({ run: loaded }), ['DonnabotDevScreen:launchd:sim'])
  assert.deepEqual(await signals({ files: { [helper(DEV, 'DonnabotDevScreen')]: null } }), ['DonnabotDevScreen:binário:sim'])
  assert.deepEqual(await signals({ files: { [helper(HOME_DEV, 'DonnabotDevScreen')]: null } }), ['DonnabotDevScreen:binário:sim'])
  // D443 (PR d): o pacote novo (Mexerica Dev.app) também conta, em /Applications e em ~/Applications.
  assert.deepEqual(await signals({ files: { [helper('/Applications/Mexerica Dev.app', 'DonnabotDevScreen')]: null } }), ['DonnabotDevScreen:binário:sim'])
  assert.deepEqual(await signals({ files: { [helper('/Users/dono/Applications/Mexerica Dev.app', 'DonnabotDevScreen')]: null } }), ['DonnabotDevScreen:binário:sim'])
  assert.deepEqual(await signals({ files: { [helper(MAIN, 'DonnabotScreen')]: null } }), ['DonnabotScreen:binário:sim'])
  assert.deepEqual(await signals({ status: () => 'enabled' }), ['agente:sim'])
  assert.deepEqual(await signals({ status: () => 'requires-approval' }), ['agente:sim'])
})

test('R1-02: erro de FS, status ausente, uid ausente ou comando incerto viram incerto', async () => {
  assert.deepEqual(await signals({ files: { [helper(DEV, 'DonnabotDevScreen')]: 'EACCES' } }), ['DonnabotDevScreen:binário:incerto'])
  assert.deepEqual(await signals({ status: () => null }), ['agente:incerto'])
  assert.deepEqual(await signals({ status: () => undefined }), ['agente:incerto'])
  assert.deepEqual(await signals({ status: () => { throw new Error('login item ilegível') } }), ['agente:incerto'])
  assert.deepEqual(await signals({ uid: null }), ['DonnabotScreen:launchd:incerto', 'DonnabotDevScreen:launchd:incerto'])
  const broken = (code: number | string, killed = false) => (bin: string) => bin === '/usr/bin/pgrep' ? { code, killed } : absent(bin)
  assert.deepEqual(await signals({ run: broken('ENOENT') }), ['DonnabotScreen:processo:incerto', 'DonnabotDevScreen:processo:incerto'])
  assert.deepEqual(await signals({ run: broken(1, true) }), ['DonnabotScreen:processo:incerto', 'DonnabotDevScreen:processo:incerto'])
})

test('R1-03: binário, registro próprio e launchd Dev surgidos durante a espera do pgrep entram na resposta', async () => {
  const wait = deferred()
  const files: Record<string, string | null> = {}
  let status = 'not-registered'
  let devLoaded = false
  const pending = loadObservers({
    files, status: () => status,
    run: async (bin, args) => {
      if (bin === '/usr/bin/pgrep') { await wait.promise; return absent(bin) }
      return devLoaded && args[1].endsWith('app.donnabot.dev.screen') ? null : absent(bin)
    },
  }).module.observerSignals()
  files[helper(DEV, 'DonnabotDevScreen')] = null
  status = 'enabled'
  devLoaded = true
  wait.resolve()
  assert.deepEqual(JSON.parse(JSON.stringify(await pending)), ['agente:sim', 'DonnabotDevScreen:binário:sim', 'DonnabotDevScreen:launchd:sim'])
})

test('R1-03: só o launchd Dev surgindo durante a espera do pgrep (bundle fora dos locais conhecidos) já é sinal', async () => {
  const wait = deferred()
  let devLoaded = false
  const pending = loadObservers({
    run: async (bin, args) => {
      if (bin === '/usr/bin/pgrep') { await wait.promise; return absent(bin) }
      return devLoaded && args[1].endsWith('app.donnabot.dev.screen') ? null : absent(bin)
    },
  }).module.observerSignals()
  devLoaded = true
  wait.resolve()
  assert.deepEqual(JSON.parse(JSON.stringify(await pending)), ['DonnabotDevScreen:launchd:sim'])
})
