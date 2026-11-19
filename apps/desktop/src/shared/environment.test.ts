// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { environmentProfile, hostEnvironmentError, isCodyncDataDir, isCodyncPort } from './environment.ts'

test('development installations cannot share production resources or update feed', () => {
  const prod = environmentProfile('main')
  const dev = environmentProfile('dev')
  assert.equal(prod.appId, 'app.donnabot.mac')
  assert.equal(prod.port, 19422)
  assert.equal(prod.dataFolder, '.mexerica')
  assert.equal(prod.hostLabel, 'app.donnabot.host')
  assert.equal(prod.supervisor, 'donnabot-hostw.exe')
  for (const key of Object.keys(prod) as (keyof typeof prod)[]) assert.notEqual(dev[key], prod[key], key)
  assert.equal(dev.releaseUpdates, false)
})

test('Donnabot never takes the resources of an installed Codync or Codync Dev', () => {
  const codync = new Set([
    'Codync', 'Codync Dev', 'com.pokai.Codync', 'com.pokai.Codync.dev', 'codync', 'codync-dev', '.codync', '.codync-dev',
    'com.pokai.codync.host', 'com.pokai.codync.dev.host', 'codync-host.service', 'codync-dev-host.service',
    'CodyncHost', 'CodyncDevHost', 'codync-hostw.exe', 'codync-dev-hostw.exe', 'com.pokai.Codync.screen', 'com.pokai.Codync.dev.screen', 'CodyncScreen', 'CodyncDevScreen',
  ])
  for (const profile of [environmentProfile('main'), environmentProfile('dev')]) {
    for (const key of ['name', 'appId', 'scheme', 'dataFolder', 'hostLabel', 'systemdUnit', 'windowsService', 'supervisor', 'screenLabel', 'screenName'] as const) {
      assert.ok(!codync.has(profile[key]), `${key}: ${profile[key]}`)
    }
    assert.ok(!isCodyncPort(profile.port))
  }
  for (const port of [19222, 19223, 19300, 19399]) assert.ok(isCodyncPort(port), String(port))
  for (const port of [19422, 19423, 19430, 19499]) assert.ok(!isCodyncPort(port), String(port))
  for (const dir of ['/Users/me/.codync', '/Users/me/.codync-dev', '/Users/me/.codync-dev-x/', '/Users/me/.codync/bots', '/Users/me/.CODYNC', '/users/ME/.Codync-Dev']) {
    assert.ok(isCodyncDataDir('/Users/me', dir), dir)
  }
  for (const dir of ['/Users/me/.donnabot', '/Users/me/.donnabot-dev-x', '/tmp/.codync', '/Users/me/work/.codync']) {
    assert.ok(!isCodyncDataDir('/Users/me', dir), dir)
  }
})

test('a development app rejects production and old unidentified hosts', () => {
  assert.equal(hostEnvironmentError('main', undefined), null)
  assert.equal(hostEnvironmentError('main', 'main'), null)
  assert.equal(hostEnvironmentError('dev', 'dev'), null)
  assert.ok(hostEnvironmentError('dev', undefined))
  assert.ok(hostEnvironmentError('dev', 'main'))
  assert.ok(hostEnvironmentError('main', 'dev'))
})
