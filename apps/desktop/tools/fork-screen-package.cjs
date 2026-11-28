const { execFileSync } = require('node:child_process')
const { existsSync, readdirSync, readFileSync, writeFileSync } = require('node:fs')
const { join } = require('node:path')

const helperName = 'DonnabotDevScreen'
const service = 'app.donnabot.dev.screen.xpc'
const signIgnore = [
  '/Contents/Library/LoginItems/DonnabotDevScreen\\.app(?:/|$)',
  '/Contents/Resources/donnabot-host$',
]

function cdhash(output) {
  const hashes = [...output.matchAll(/^CDHash=(.*)$/gm)].map((match) => match[1])
  if (hashes.length !== 1 || !/^[0-9a-f]{40}$/.test(hashes[0])) throw new Error('CDHash inválido ou ausente.')
  return hashes[0]
}

function codesign(...args) {
  return execFileSync('/usr/bin/codesign', args, { timeout: 15_000, stdio: 'pipe' })
}

function displayHash(path, { requireRuntime = false } = {}) {
  const { spawnSync } = require('node:child_process')
  const result = spawnSync('/usr/bin/codesign', ['--display', '--verbose=4', path], {
    encoding: 'utf8', timeout: 15_000, stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (result.status !== 0 || result.error) throw new Error('Não foi possível ler o CDHash do executável.')
  if (requireRuntime && !/^CodeDirectory .*flags=0x[0-9a-f]+\([^)]*\bruntime\b/m.test(result.stderr)) {
    throw new Error('O Hub precisa manter a proteção Hardened Runtime.')
  }
  return cdhash(result.stderr)
}

function paths({ appOutDir, packager }) {
  const contents = join(appOutDir, `${packager.appInfo.productFilename}.app`, 'Contents')
  const helper = join(contents, 'Library/LoginItems', `${helperName}.app`)
  return {
    helper,
    agent: join(contents, 'Library/LaunchAgents/app.donnabot.dev.screen.plist'),
    hub: join(contents, 'Resources/donnabot-host'),
    pin: join(contents, 'Resources/screen-identity.cdhashes'),
  }
}

function readVerifiedPins({ helper, hub }) {
  codesign('--verify', '--deep', '--strict', helper)
  codesign('--verify', '--strict', hub)
  const screenHash = displayHash(join(helper, 'Contents/MacOS', helperName))
  const hubHash = displayHash(hub, { requireRuntime: true })
  if (screenHash === hubHash) throw new Error('O helper e o Hub precisam ter identidades distintas.')
  return `${screenHash}\n${hubHash}\n`
}

function afterPack(context) {
  if (context.electronPlatformName !== 'darwin') throw new Error('O helper autenticado exige macOS.')
  const bundle = paths(context)
  const add = (file, key, type, value) => execFileSync('/usr/bin/plutil', ['-insert', key, type, value, file], { timeout: 15_000 })
  add(join(bundle.helper, 'Contents/Info.plist'), 'MexericaScreenService', '-string', service)
  add(bundle.agent, 'MachServices', '-json', JSON.stringify({ [service]: true }))
  const sign = (path) => codesign('--force', '--sign', '-', '--timestamp=none', path)
  const frameworks = join(bundle.helper, 'Contents/Frameworks')
  if (existsSync(frameworks)) {
    for (const entry of readdirSync(frameworks)) sign(join(frameworks, entry))
  }
  sign(bundle.helper)
  codesign('--force', '--sign', '-', '--options', 'runtime', '--timestamp=none', bundle.hub)
  writeFileSync(bundle.pin, readVerifiedPins(bundle), { flag: 'wx' })
}

function afterSign(context) {
  const bundle = paths(context)
  codesign('--verify', '--deep', '--strict', join(bundle.hub, '../../..'))
  if (readFileSync(bundle.pin, 'utf8') !== readVerifiedPins(bundle)) {
    throw new Error('A assinatura final não preservou a identidade do helper ou a do Hub.')
  }
}

module.exports = { afterPack, afterSign, cdhash, signIgnore }
