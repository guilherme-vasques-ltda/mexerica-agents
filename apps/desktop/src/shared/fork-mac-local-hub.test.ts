import assert from 'node:assert/strict'
import test from 'node:test'
import { THROTTLE_SECONDS, expectedProgramArguments, hubLocation, launchdServiceAbsent, localHostOff, macPlistValid, parseProgramArguments, uninstallArgs } from './fork-mac-local-hub.ts'

const prefs = (data: Record<string, unknown>) => (key: string) => data[key]

test('sem escolha o Hub local fica disabled, mesmo com a chave de build ligada', () => {
  assert.equal(hubLocation(prefs({})), 'remote')
  assert.equal(localHostOff({ feature: true, isMac: true, devPort: null, read: prefs({}) }), true)
  assert.equal(localHostOff({ feature: true, isMac: true, devPort: null, read: prefs({ hubLocation: 'x' }) }), true)
})

test('só chave de build, Mac e escolha "mac" juntas ligam o Hub local', () => {
  const read = prefs({ hubLocation: 'mac' })
  assert.equal(localHostOff({ feature: true, isMac: true, devPort: null, read }), false)
  assert.equal(localHostOff({ feature: false, isMac: true, devPort: null, read }), true)
  assert.equal(localHostOff({ feature: true, isMac: false, devPort: null, read }), true)
})

test('a escolha é lida a cada chamada: mudar a preferência vale na hora', () => {
  const data: Record<string, unknown> = {}
  const inputs = { feature: true, isMac: true, devPort: null, read: prefs(data) }
  assert.equal(localHostOff(inputs), true)
  data.hubLocation = 'mac'
  assert.equal(localHostOff(inputs), false)
  data.hubLocation = 'remote'
  assert.equal(localHostOff(inputs), true)
})

test('host de desenvolvimento continua valendo', () => {
  assert.equal(localHostOff({ feature: false, isMac: false, devPort: 4000, read: prefs({}) }), false)
})

test('desinstalação níveis 1 e 2 passam o nível ao Hub', () => {
  assert.deepEqual(uninstallArgs(1), ['uninstall', '--nivel', '1'])
  assert.deepEqual(uninstallArgs(2), ['uninstall', '--nivel', '2'])
  // Windows mantém o caminho herdado, sem --nivel.
  assert.deepEqual(uninstallArgs(1, true), ['uninstall'])
})

const plistWith = (args: string[], throttle: number = THROTTLE_SECONDS) =>
  `<plist><dict><key>ProgramArguments</key>\n<array>\n${args.map((a) => `  <string>${a}</string>`).join('\n')}\n</array><key>ThrottleInterval</key><integer>${throttle}</integer></dict></plist>`

test('plist do modo só-loopback vale; o herdado com o mesmo caminho e sem as flags não', () => {
  const exe = '/Applications/Mexerica.app/Contents/Resources/donnabot-host'
  assert.equal(macPlistValid(plistWith(expectedProgramArguments(exe, 19422)), [exe], 19422), true)
  assert.equal(macPlistValid(plistWith([exe, 'serve', '--port', '19422']), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith([exe, 'serve', '--port', '19422', '--bind', '0.0.0.0']), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith([exe, 'serve', '--port', '19422', '--bind', '127.0.0.1']), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith([...expectedProgramArguments(exe, 19422), '--extra']), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith(expectedProgramArguments(exe, 1)), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith(expectedProgramArguments('/outro', 19422)), [exe], 19422), false)
  assert.equal(macPlistValid(plistWith(expectedProgramArguments('/real/donnabot-host', 19422)), [exe, '/real/donnabot-host'], 19422), true)
  assert.equal(macPlistValid(plistWith(expectedProgramArguments(exe, 19422), 10), [exe], 19422), false)
  assert.equal(THROTTLE_SECONDS, 30)
  assert.equal(macPlistValid(null, [exe], 19422), false)
  assert.equal(macPlistValid('<plist></plist>', [exe], 19422), false)
})

test('o plist é lido com escapes XML', () => {
  assert.deepEqual(parseProgramArguments(plistWith(['/a&amp;b/&lt;x&gt;', 'serve'])), ['/a&b/<x>', 'serve'])
})

test('launchctl print só conta como ausência pelo código 113 ou pela mensagem de serviço inexistente', () => {
  assert.equal(launchdServiceAbsent(113, ''), true)
  assert.equal(launchdServiceAbsent(1, 'Could not find service "x" in domain for user gui: 501'), true)
  assert.equal(launchdServiceAbsent(0, ''), false)
  assert.equal(launchdServiceAbsent(5, 'Operation not permitted'), false)
})
