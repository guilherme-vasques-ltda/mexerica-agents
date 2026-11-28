// electron-builder merges inherited arrays, which would let Dev register production callbacks.
// Compose the two objects ourselves so variant-owned resource and protocol lists replace them.
const { readFileSync } = require('node:fs')
const { join } = require('node:path')
const { parse } = require('yaml')
const root = join(__dirname, '..')
const base = parse(readFileSync(join(root, 'electron-builder.yml'), 'utf8'))
const dev = parse(readFileSync(join(root, 'electron-builder.dev.yml'), 'utf8'))
const pinnedScreen = process.env.MEXERICA_PIN_SCREEN_HELPER === '1' ? require('./fork-screen-package.cjs') : null
module.exports = {
  ...base,
  ...dev,
  mac: { ...base.mac, ...dev.mac, ...(pinnedScreen ? { signIgnore: [...dev.mac.signIgnore, ...pinnedScreen.signIgnore] } : {}) },
  ...(pinnedScreen ? { afterPack: pinnedScreen.afterPack, afterSign: pinnedScreen.afterSign } : {}),
  dmg: { ...base.dmg, ...dev.dmg },
  linux: { ...base.linux, ...dev.linux },
  win: { ...base.win, ...dev.win },
}
