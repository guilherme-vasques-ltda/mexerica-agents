// O aviso de motor mudo na conversa — leitura do dado do Hub e frase em cada idioma do app.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '@shared/models'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { lerMotorMudo, textoMotorMudo } from './motor-mudo.ts'

const IDIOMAS = ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const

const aviso = (motorMudo: unknown): Entry =>
  ({ id: 'n1', seq: 1, botId: 'b', rev: 1, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, data: { author: 'b', style: 'info', motorMudo } }) as Entry

test('lê o estado e arredonda os segundos para minutos, nunca abaixo de um', () => {
  assert.deepEqual(lerMotorMudo(aviso({ estado: 'mudo', segundos: 600 })), { estado: 'mudo', minutos: 10 })
  assert.deepEqual(lerMotorMudo(aviso({ estado: 'voltou', segundos: 2 })), { estado: 'voltou', minutos: 1 })
})

test('linha comum e dado de forma estranha não viram aviso de motor mudo', () => {
  assert.equal(lerMotorMudo(aviso(undefined)), null)
  assert.equal(lerMotorMudo(aviso({ estado: 'outro', segundos: 600 })), null)
  assert.equal(lerMotorMudo(aviso({ estado: 'mudo', segundos: 'dez' })), null)
})

test('a frase sai no idioma do app, com o tempo, e a de "voltou" não promete nada', () => {
  const pt = tradutor('pt-BR')
  assert.equal(textoMotorMudo(pt, { estado: 'mudo', minutos: 10 }), 'O bot está há 10 minutos sem dar sinal. Se parecer travado, use Parar; se não, é só esperar.')
  assert.equal(textoMotorMudo(tradutor('en'), { estado: 'mudo', minutos: 1 }), 'The bot has shown no sign of activity for 1 minute. If it looks stuck, use Stop; otherwise just wait.')
  for (const idioma of IDIOMAS) {
    const t = tradutor(idioma)
    assert.ok(textoMotorMudo(t, { estado: 'mudo', minutos: 10 }).length > 10, idioma)
    assert.ok(!textoMotorMudo(t, { estado: 'mudo', minutos: 10 }).includes('{'), idioma)
    assert.ok(textoMotorMudo(t, { estado: 'voltou', minutos: 10 }).length > 10, idioma)
  }
})
