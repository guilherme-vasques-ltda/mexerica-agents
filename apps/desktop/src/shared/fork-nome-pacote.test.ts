import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import * as path from 'node:path'
import { test } from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'
import { pacotesDoApp } from './fork-nome-pacote.ts'
const require = createRequire(import.meta.url)
const { parse } = require('yaml')

// D443 (PR d), D70: o pacote instalado passa a se chamar Mexerica, mas a identidade técnica não muda. É ela que
// guarda os dados de quem já usa: `app.setName` dá o nome do item do Chaveiro ("<nome> Safe Storage") e da trava de
// instância única, e `userData` é a pasta em Application Support. Se qualquer um mudar, a sessão salva e o
// personal-pin se perdem (PLANO §0, itens 1 e 2).
const APP_DATA = '/Users/dono/Library/Application Support'

function ambiente(environment: 'main' | 'dev') {
  const chamadas: string[] = []
  const pastas: { path: string; create: boolean }[] = []
  let userData = ''
  const modulo = loadMainModule<typeof import('../main/environment.ts')>('environment', {
    'node:crypto': { createHash },
    'node:fs': { readFileSync: () => JSON.stringify({ environment }) },
    'node:path': path,
    'node:os': { homedir: () => '/home/usuario' },
    './fork-keychain/generation': { readStorageGeneration: () => 0, keychainFailure: () => 'unsupported' },
    '../shared/fork-private-files': { privateDirectory: (pasta: string, create: boolean) => {
      pastas.push({ path: pasta, create })
      return pasta
    } },
    electron: { app: {
      isPackaged: true,
      getName: () => environment === 'dev' ? 'Mexerica Dev' : 'Mexerica',
      getVersion: () => '0.0.1',
      getAppPath: () => '/Applications/Mexerica.app/Contents/Resources/app.asar',
      getPath: (nome: string) => {
        if (nome === 'userData') { assert.notEqual(userData, ''); return userData }
        assert.equal(nome, 'appData')
        return APP_DATA
      },
      setName: (nome: string) => chamadas.push(`setName ${nome}`),
      setPath: (nome: string, valor: string) => {
        assert.equal(nome, 'userData')
        userData = valor
        chamadas.push(`setPath ${nome} ${valor}`)
      },
    } },
  }, { process: { resourcesPath: '/Applications/Mexerica.app/Contents/Resources', platform: 'darwin', env: {} } })
  const dados = `/home/usuario/${environment === 'dev' ? '.mexerica-dev' : '.mexerica'}`
  assert.equal(modulo.hostDataDir, dados)
  assert.deepEqual(pastas, [{ path: dados, create: false }])
  return chamadas
}

test('D443 (bloqueante): com o pacote Mexerica, app.setName e userData continuam Donnabot (Chaveiro "Donnabot Safe Storage")', () => {
  assert.deepEqual(ambiente('main'), ['setName Donnabot', `setPath userData ${APP_DATA}/Donnabot`])
  assert.deepEqual(ambiente('dev'), ['setName Donnabot Dev', `setPath userData ${APP_DATA}/Donnabot Dev`])
})

test('D443: só o nome do pacote muda; appId, esquemas e o nome do pacote npm ficam', () => {
  const prod = parse(readFileSync(new URL('../../electron-builder.yml', import.meta.url), 'utf8'))
  const dev = parse(readFileSync(new URL('../../electron-builder.dev.yml', import.meta.url), 'utf8'))
  const pkg = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8'))
  assert.equal(prod.productName, 'Mexerica')
  assert.equal(dev.productName, 'Mexerica Dev')
  assert.equal(pkg.productName, 'Mexerica')
  assert.equal(pkg.name, 'donnabot-desktop')
  assert.equal(prod.appId, 'app.donnabot.mac')
  assert.equal(dev.appId, 'app.donnabot.mac.dev')
  assert.deepEqual(prod.protocols[0].schemes, ['donnabot', 'app.donnabot.mac'])
  assert.deepEqual(dev.protocols[0].schemes, ['donnabot-dev', 'app.donnabot.mac.dev'])
  assert.equal(prod.mac.artifactName, 'mexerica-macos.${ext}')
  assert.equal(prod.dmg.artifactName, 'mexerica-macos.dmg')
  // O bundle se procura pelo nome novo e pelo antigo (instalação de antes ou rollback).
  assert.deepEqual(pacotesDoApp('main'), ['Mexerica', 'Donnabot'])
  assert.deepEqual(pacotesDoApp('dev'), ['Mexerica Dev', 'Donnabot Dev'])
})
