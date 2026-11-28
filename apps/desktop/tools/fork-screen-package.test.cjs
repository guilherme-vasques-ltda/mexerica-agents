const { strict: assert } = require('node:assert')
const { test } = require('node:test')
const { cdhash } = require('./fork-screen-package.cjs')
const { execFileSync } = require('node:child_process')
const { join } = require('node:path')

test('lê um único CDHash canônico do codesign', () => {
  const hash = '1234567890abcdef1234567890abcdef12345678'
  const valid = `Identifier=example.invalid\nCDHash=${hash}\n`
  assert.equal(cdhash(valid), hash)
  for (const invalid of ['', `${valid}CDHash=inválido\n`, `CDHash=${hash.toUpperCase()}\n`, 'CDHash=abcdef\n']) {
    assert.throws(() => cdhash(invalid))
  }
})

test('compila o helper nativo de tela', { skip: process.platform !== 'darwin' }, () => {
  execFileSync(process.execPath, [join(__dirname, 'native-screen.mjs'), 'dev'], { stdio: 'inherit' })
})
