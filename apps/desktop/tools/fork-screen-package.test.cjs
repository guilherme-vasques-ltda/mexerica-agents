const { strict: assert } = require('node:assert')
const { test } = require('node:test')
const { cdhash } = require('./fork-screen-package.cjs')
const { execFileSync } = require('node:child_process')
const { mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs')
const { tmpdir } = require('node:os')
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

test('o helper só aceita o alvo verificado uma vez e dentro do prazo', { skip: process.platform !== 'darwin' }, () => {
  const temp = mkdtempSync(join(tmpdir(), 'screen-click-'))
  try {
    const main = join(temp, 'main.swift')
    writeFileSync(main, 'import Foundation\n' + readFileSync(join(__dirname, 'fork-local-click-fixture.swift'), 'utf8'))
    const code = join(temp, 'click')
    execFileSync('xcrun', ['swiftc', join(__dirname, '../../screen-macos/fork-local-click.swift'), main, '-o', code], { stdio: 'inherit' })
    execFileSync(code, { stdio: 'inherit' })
  } finally {
    rmSync(temp, { recursive: true, force: true })
  }
})
