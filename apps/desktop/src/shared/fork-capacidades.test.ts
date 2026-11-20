import assert from 'node:assert/strict'
import test from 'node:test'
import { createWebBridge } from '../web/platform.ts'
import { capacidades } from './fork-capacidades.ts'

const semSuporte = {
  dock: false,
  launchAtLogin: false,
  appIcon: false,
  ssh: false,
  cofreLocal: false,
  perfilConta: false,
}

test('sem argumento, todas as capacidades ficam desabilitadas', () => {
  assert.deepEqual(capacidades(), semSuporte)
})

for (const [nome, bridge] of [
  ['undefined', undefined],
  ['null', null],
  ['campos ausentes', {}],
  ['plataforma ausente', { web: true }],
  ['plataforma desconhecida', { platform: 'freebsd' }],
  ['plataforma vazia', { platform: '' }],
] as const) {
  test(`ponte com ${nome} desabilita as capacidades sem lançar`, () => {
    assert.deepEqual(capacidades(bridge), semSuporte)
  })
}

test('Web desabilita capacidades nativas mesmo com os métodos dos stubs presentes', () => {
  assert.deepEqual(capacidades(createWebBridge()), semSuporte)
})

for (const platform of ['darwin', 'linux', 'win32'] as const) {
  test(`Desktop ${platform} habilita o que a plataforma suporta sem o marcador Web`, () => {
    assert.deepEqual(capacidades({ platform }), {
      dock: platform === 'darwin',
      launchAtLogin: true,
      appIcon: true,
      ssh: true,
      cofreLocal: true,
      perfilConta: true,
    })
  })

  test(`o marcador Web prevalece sobre a plataforma ${platform}`, () => {
    assert.deepEqual(capacidades({ web: true, platform }), semSuporte)
  })
}

test('Open não oferece as três preferências nativas; Main e Dev mantêm as opções do Mac', () => {
  for (const appScheme of ['mexerica-open', 'donnabot', 'donnabot-dev']) {
    const cap = capacidades({ platform: 'darwin', appScheme })
    const disponivel = appScheme !== 'mexerica-open'
    assert.equal(cap.dock, disponivel)
    assert.equal(cap.launchAtLogin, disponivel)
    assert.equal(cap.appIcon, disponivel)
  }
})
