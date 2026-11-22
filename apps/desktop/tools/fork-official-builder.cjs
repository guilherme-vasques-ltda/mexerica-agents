const { lstatSync, readFileSync } = require('node:fs')
const { join } = require('node:path')
const { parse } = require('yaml')

const base = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8'))
const stage = 'build/official'
const out = `${stage}/out`
const { background: _background, ...dmg } = base.dmg
const resources = {
  darwin: ['donnabot-host', 'codync-speech'],
  win32: ['donnabot-host.exe', 'donnabot-hostw.exe'],
  linux: ['donnabot-host'],
}
const native = (platform) => resources[platform].map(name => ({ from: `build/native/${name}`, to: name }))

async function verifyOfficial(context) {
  const project = context.packager.projectDir
  const { officialAccount, officialConfigHash } = await import('../src/shared/fork-official-account.ts')
  try {
    const config = officialAccount(JSON.parse(readFileSync(join(project, stage, 'account-config.json'), 'utf8')))
    if (!config || officialConfigHash(config) !== readFileSync(join(project, out, 'main/official-config.sha256'), 'utf8')) throw new Error()
  } catch { throw new Error('Official account configuration does not match the compiled desktop.') }
  const platform = context.electronPlatformName
  if (!Object.hasOwn(resources, platform)) throw new Error('Unsupported official desktop platform.')
  if (platform === 'darwin') {
    try {
      const symbols = JSON.parse(readFileSync(join(project, out, 'renderer/symbols/manifest.json'), 'utf8'))
      if (!symbols || typeof symbols !== 'object' || Array.isArray(symbols) || Object.hasOwn(symbols, 'apple.logo')) throw new Error()
    } catch { throw new Error('Missing or invalid macOS desktop symbols.') }
  }
  for (const filename of resources[platform]) {
    const stat = lstatSync(join(project, 'build/native', filename), { throwIfNoEntry: false })
    if (!stat?.isFile() || stat.size === 0 || (platform !== 'win32' && !(stat.mode & 0o111))) {
      throw new Error(`Missing or invalid official desktop resource: build/native/${filename}`)
    }
  }
}

module.exports = {
  ...base,
  npmRebuild: false,
  directories: { ...base.directories, output: 'dist/official' },
  files: [{ from: out, to: 'out' }, 'package.json'],
  extraResources: base.extraResources.map(entry => entry.to === 'account-config.json' ? { ...entry, from: `${stage}/account-config.json` } : entry),
  electronFuses: {
    runAsNode: false,
    enableNodeOptionsEnvironmentVariable: false,
    enableNodeCliInspectArguments: false,
    onlyLoadAppFromAsar: true,
  },
  mac: { ...base.mac, extraFiles: [], signIgnore: [], identity: '-', timestamp: 'none', hardenedRuntime: false, notarize: false, extraResources: native('darwin') },
  dmg,
  win: { ...base.win, extraResources: [...native('win32'), { from: 'resources/icon-tray.png', to: 'icon-tray.png' }] },
  linux: { ...base.linux, target: ['deb', 'AppImage'], extraResources: native('linux') },
  beforePack: verifyOfficial,
}
