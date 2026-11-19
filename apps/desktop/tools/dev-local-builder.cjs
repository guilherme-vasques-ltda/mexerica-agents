// The Dev variant of the existing local client package: no Host or screen helper.
const { readFileSync } = require('node:fs')
const { join } = require('node:path')
const { parse } = require('yaml')
const local = require('./local-builder.cjs')
const dev = parse(readFileSync(join(__dirname, '..', 'electron-builder.dev.yml'), 'utf8'))
module.exports = {
  ...local,
  appId: dev.appId,
  productName: dev.productName,
  extraMetadata: dev.extraMetadata,
  protocols: dev.protocols,
}
