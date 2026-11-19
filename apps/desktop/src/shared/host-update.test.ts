import assert from 'node:assert/strict'
import test from 'node:test'
import { hostUpdateFixture } from './test-support/host-update.ts'
import { settle } from './test-support/main-module.ts'

for (const platform of ['darwin', 'win32']) {
  test(`${platform}: updating stops the bundled host after unregistering screen and persists restart`, async () => {
    const f = hostUpdateFixture({ platform })
    await f.host.prepareForUpdate()
    await settle()
    assert.deepEqual(f.order, ['unregisterScreen', 'stop'])
    assert.equal(f.persisted.hostRestartAfterAppUpdate, true)
    await f.host.refresh()
    assert.deepEqual(f.calls, [['stop']], 'refresh cannot restart the host during replacement')
    assert.equal(await f.host.isIdleForUpdate(), false)
  })
}

for (const options of [{ platform: 'linux' }, { devPort: '19433' }, { devPort: '19433', devPortVar: 'MEXERICA_PORT' }]) {
  test(`app replacement leaves externally managed hosts running: ${JSON.stringify(options)}`, async () => {
    const f = hostUpdateFixture(options)
    await f.host.prepareForUpdate()
    assert.equal(f.calls.length, 0)
    assert.equal(f.order.length, 0)
    assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), false)
  })
}

test('an install already in progress prevents update preparation from stopping the host', async () => {
  const f = hostUpdateFixture()
  const installing = f.host.install()
  await settle()
  await assert.rejects(f.host.prepareForUpdate(), /being installed/)
  assert.equal(f.calls.some((args) => args[0] === 'stop'), false)
  await f.tick(1000)
  await installing
})

test('missing bundled binary rejects preparation without pretending stop succeeded', async () => {
  const f = hostUpdateFixture({ missingBinary: true })
  await assert.rejects(f.host.prepareForUpdate(), /bundled host is missing/)
  assert.equal(f.calls.length, 0)
})

test('failed stop reports the error and cancellation reinstalls the service once', async () => {
  const f = hostUpdateFixture()
  f.behavior.stopStatus = 1
  await assert.rejects(f.host.prepareForUpdate(), /service command failed/)
  f.host.resumeAfterCancelledUpdate()
  f.host.resumeAfterCancelledUpdate()
  await settle()
  assert.deepEqual(f.calls, [['stop'], ['install', '--port', '19422']])
  await f.tick(1000)
})

test('new app launch reinstalls the host and clears restart marker only after health matches', async () => {
  const f = hostUpdateFixture({ restartMarker: true })
  const starting = f.host.refresh()
  await settle()
  assert.deepEqual(f.calls, [['install', '--port', '19422']])
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), true)
  f.behavior.reachable = false
  await f.tick(1000)
  await starting
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), true)
  f.behavior.reachable = true
  // Antes de anexar a primeira vez o retry vem no ritmo do backoff (250 ms), não na vigia de 5 s.
  await f.tick(250)
  assert.equal(f.host.state.kind, 'running')
  assert.equal(f.host.local?.computerId, 'fixture-computer')
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), false)
})

test('a stale host binary cannot satisfy the restart marker', async () => {
  const f = hostUpdateFixture({ restartMarker: true })
  f.behavior.wrongBinary = true
  const starting = f.host.refresh()
  await settle()
  await f.tick(1000)
  await starting
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), true)
  assert.notEqual(f.host.state.kind, 'running')
})

test('failed service installation keeps the restart marker and exposes the error', async () => {
  const f = hostUpdateFixture({ restartMarker: true })
  f.behavior.installStatus = 1
  await f.host.refresh()
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), true)
  assert.equal(f.host.state.kind, 'failed')
  if (f.host.state.kind === 'failed') assert.match(f.host.state.message, /service command failed/)
})

test('an uninstalled host does not acquire a restart marker', async () => {
  const f = hostUpdateFixture({ installed: false })
  await f.host.prepareForUpdate()
  assert.equal(f.prefs.get('hostRestartAfterAppUpdate'), false)
})

test('idle detection distinguishes working, offline and idle hosts', async () => {
  const f = hostUpdateFixture()
  assert.equal(await f.host.isIdleForUpdate(), true)
  f.behavior.busy = true
  assert.equal(await f.host.isIdleForUpdate(), false)
  f.behavior.reachable = false
  assert.equal(await f.host.isIdleForUpdate(), false)
})


test('development install and stop reject a production binary before touching its service', async () => {
  const f = hostUpdateFixture({ environment: 'dev' })
  f.behavior.binaryEnvironment = 'main'
  await f.host.install()
  assert.equal(f.host.state.kind, 'failed')
  assert.deepEqual(f.calls, [])
  assert.equal(await f.host.uninstall(), false)
  assert.deepEqual(f.calls, [])
})

test('development service installs on its own port', async () => {
  const f = hostUpdateFixture({ environment: 'dev' })
  const installing = f.host.install()
  await settle()
  assert.deepEqual(f.calls, [['install', '--port', '19423']])
  await f.tick(1000)
  await installing
  assert.equal(f.host.state.kind, 'running')
})

test('D76: sem host local, o app nunca instala nem inicia o host', async () => {
  for (const platform of ['darwin', 'linux']) {
    const f = hostUpdateFixture({ localHost: false, installed: false, platform })
    await f.host.refresh()
    await f.host.install()
    f.host.restart()
    await f.tick(1000)
    await f.tick(5000)
    assert.deepEqual(f.calls, [])
    assert.equal(f.host.state.kind, 'disabled')
    assert.equal(f.host.local, null)
  }
})

test('D76: um LaunchAgent do host de um build anterior sai pelo label, sem o binário', async () => {
  const f = hostUpdateFixture({ localHost: false, missingBinary: true })
  await f.host.refresh()
  assert.deepEqual(f.calls, [['bootout', 'gui/501/app.donnabot.host']])
  assert.deepEqual(f.removed, ['/home/usuario/Library/LaunchAgents/app.donnabot.host.plist'])
  assert.equal(f.host.state.kind, 'disabled')
  const reset = hostUpdateFixture({ localHost: false, missingBinary: true })
  assert.equal(await reset.host.uninstall(), true)
  assert.equal(reset.removed.length, 1)
  // No Linux, o mesmo usuário pode estar rodando o próprio Hub: o serviço fica.
  const linux = hostUpdateFixture({ localHost: false, platform: 'linux' })
  await linux.host.refresh()
  assert.equal(await linux.host.uninstall(), true)
  assert.deepEqual([linux.calls, linux.removed], [[], []])
})

test('macLocalHub: sem escolha fica disabled e nunca instala; com a escolha instala só com --port', async () => {
  const sem = hostUpdateFixture({ localHost: false, macLocalHub: true, installed: false })
  await sem.host.refresh()
  await sem.host.install()
  assert.deepEqual(sem.calls, [])
  assert.equal(sem.host.state.kind, 'disabled')
  const com = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac', installed: false })
  const starting = com.host.refresh()
  await settle()
  assert.deepEqual(com.calls, [['install', '--port', '19422']])
  await com.tick(1000)
  await starting
  assert.deepEqual(com.calls.flat().filter((arg) => arg.includes('0.0.0.0') || arg.includes('--bind')), [])
})

test('macLocalHub: a escolha é lida a cada chamada e desfazer remove o serviço na hora', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac', installed: false })
  const installing = f.host.install()
  await settle()
  await f.tick(1000)
  await installing
  assert.deepEqual(f.calls, [['install', '--port', '19422']])
  f.behavior.installed = true // o plist que o install escreveu
  f.prefs.set('hubLocation', 'remote')
  await f.host.refresh()
  assert.deepEqual(f.calls[1], ['bootout', 'gui/501/app.donnabot.host'])
  assert.equal(f.host.state.kind, 'disabled')
  assert.equal(f.removed.length, 1)
})

test('macLocalHub: desinstalar nível 1 e 2 passam --nivel ao Hub; sem chave de build nada muda', async () => {
  for (const level of [1, 2] as const) {
    const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac' })
    assert.equal(await f.host.uninstall(level), true)
    assert.deepEqual(f.calls, [['uninstall', '--nivel', String(level)]])
  }
  const off = hostUpdateFixture({ localHost: false, macLocalHub: false, hubLocation: 'mac', installed: false })
  await off.host.refresh()
  assert.deepEqual(off.calls, [])
  assert.equal(off.host.state.kind, 'disabled')
})

test('macLocalHub: plist herdado com o mesmo caminho e sem --bind/--only-this-mac é reinstalado, não aceito', async () => {
  const bin = '/bundle/donnabot-host'
  for (const args of [[bin, 'serve', '--port', '19422'], [bin, 'serve', '--port', '19422', '--bind', '0.0.0.0']]) {
    const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac' })
    f.behavior.plistArgs = args
    f.behavior.onInstall = () => { f.behavior.plistArgs = null }
    const refreshing = f.host.refresh()
    await settle()
    assert.deepEqual(f.calls, [['install', '--port', '19422']])
    await f.tick(1000)
    await refreshing
    assert.equal(f.host.state.kind, 'running')
  }
  const ok = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac' })
  await ok.host.refresh()
  await settle()
  assert.deepEqual(ok.calls, [])
})

test('macLocalHub: se o install não deixa o plist só-loopback, o estado final é falha', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac', installed: false })
  f.behavior.onInstall = () => { f.behavior.plistArgs = ['/bundle/donnabot-host', 'serve', '--port', '19422'] }
  const installing = f.host.install()
  await settle()
  await installing
  assert.equal(f.host.state.kind, 'failed')
})

test('macLocalHub: bootout que falha com o serviço ainda carregado não apaga o plist nem confirma a remoção', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'remote' })
  f.behavior.bootoutStatus = 5
  await f.host.refresh()
  assert.deepEqual(f.removed, [])
  assert.equal(f.host.state.kind, 'failed')
  assert.equal(await f.host.uninstall(), false)
  assert.deepEqual(f.removed, [])
  // bootout falhou mas o serviço já estava ausente: é remoção, não erro.
  const gone = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'remote' })
  gone.behavior.bootoutStatus = 5
  gone.behavior.loaded = false
  await gone.host.refresh()
  assert.equal(gone.host.state.kind, 'disabled')
  assert.equal(gone.removed.length, 1)
  // falha ao apagar o plist também propaga.
  const rm = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'remote' })
  rm.behavior.rmFails = true
  assert.equal(await rm.host.uninstall(), false)
  assert.equal(rm.host.state.kind, 'failed')
})

test('macLocalHub: serviço CARREGADO sem o arquivo plist (versão antiga) também sai, e falha propaga', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'remote', installed: false })
  f.behavior.loaded = true
  await f.host.refresh()
  assert.deepEqual(f.calls, [['bootout', 'gui/501/app.donnabot.host']])
  assert.equal(f.host.state.kind, 'disabled')
  const stuck = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'remote', installed: false })
  stuck.behavior.loaded = true
  stuck.behavior.bootoutStatus = 5
  await stuck.host.refresh()
  assert.equal(stuck.host.state.kind, 'failed')
  assert.equal(await stuck.host.uninstall(), false)
})

test('macLocalHub: corrida da preferência durante o verifyBinary não instala nada', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac', installed: false })
  const installing = f.host.install()
  f.prefs.set('hubLocation', 'remote') // o usuário muda enquanto o `compat` roda
  const refreshing = f.host.refresh()
  await settle()
  await refreshing
  await installing
  await f.tick(1000)
  assert.deepEqual(f.calls, [])
  assert.equal(f.host.state.kind, 'disabled')
})

test('macLocalHub: preferência muda durante o install: o serviço criado é removido e o estado final é disabled', async () => {
  const f = hostUpdateFixture({ localHost: false, macLocalHub: true, hubLocation: 'mac', installed: false })
  f.behavior.onInstall = () => { f.prefs.set('hubLocation', 'remote') }
  const installing = f.host.install()
  await settle()
  await installing
  assert.deepEqual(f.calls.map((c) => c[0]), ['install', 'bootout'])
  assert.equal(f.removed.length, 1)
  assert.equal(f.host.state.kind, 'disabled')
})

test('#915 MEXERICA_PORT vence DONNABOT_PORT sem aviso; só a antiga avisa e continua valendo', () => {
  const novo = hostUpdateFixture({ env: { MEXERICA_PORT: '19433', DONNABOT_PORT: '19999' } })
  assert.equal(novo.warnings.length, 0)
  assert.equal(novo.devPort, 19433)
  const velho = hostUpdateFixture({ devPort: '19433' })
  assert.equal(velho.devPort, 19433)
  assert.equal(velho.warnings.length, 1)
  assert.match(velho.warnings[0], /DONNABOT_PORT.*MEXERICA_PORT/)
})

const NOVO = '/novo/donnabot-host'
const VELHO = '/velho/donnabot-host'
const hostBin = (env: Record<string, string>) => hostUpdateFixture({ platform: 'linux', missingBinary: true, existing: [NOVO, VELHO], env })

test('#915 HostController.binary: MEXERICA_HOST_BIN vence a antiga; só a antiga continua valendo e avisa uma vez', () => {
  const ambas = hostBin({ MEXERICA_HOST_BIN: NOVO, DONNABOT_HOST_BIN: VELHO })
  assert.equal(ambas.host.binary, NOVO)
  assert.equal(ambas.warnings.length, 0)
  const antiga = hostBin({ DONNABOT_HOST_BIN: VELHO })
  assert.equal(antiga.host.binary, VELHO)
  assert.equal(antiga.host.binary, VELHO)
  assert.equal(antiga.warnings.length, 1, 'aviso único mesmo lendo o getter várias vezes')
  assert.match(antiga.warnings[0], /DONNABOT_HOST_BIN.*MEXERICA_HOST_BIN/)
  const vazia = hostBin({ MEXERICA_HOST_BIN: '', DONNABOT_HOST_BIN: VELHO })
  assert.equal(vazia.host.binary, VELHO)
  assert.equal(vazia.warnings.length, 1)
  assert.equal(hostBin({}).host.binary, null)
})
