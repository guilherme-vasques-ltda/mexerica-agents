import assert from 'node:assert/strict'
import test from 'node:test'
import { createWebBridge } from '../web/platform.ts'
import { capacidades } from './fork-capacidades.ts'

const semSuporte = {
  launchAtLogin: false,
  appIcon: false,
  ssh: false,
  cofreLocal: false,
  perfilConta: false,
  atalhoVoz: false,
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
  test(`ponte com ${nome} desabilita todas as capacidades sem lançar`, () => {
    assert.deepEqual(capacidades(bridge), semSuporte)
  })
}

test('Web desabilita capacidades nativas mesmo com os métodos dos stubs presentes', () => {
  assert.deepEqual(capacidades(createWebBridge()), semSuporte)
})

for (const platform of ['darwin', 'linux', 'win32'] as const) {
  test(`Desktop ${platform} habilita todas as capacidades sem o marcador Web`, () => {
    assert.deepEqual(capacidades({ platform }), {
      launchAtLogin: true,
      appIcon: true,
      ssh: true,
      cofreLocal: true,
      perfilConta: true,
      atalhoVoz: true,
    })
  })

  test(`o marcador Web prevalece sobre a plataforma ${platform}`, () => {
    assert.deepEqual(capacidades({ web: true, platform }), semSuporte)
  })
}
