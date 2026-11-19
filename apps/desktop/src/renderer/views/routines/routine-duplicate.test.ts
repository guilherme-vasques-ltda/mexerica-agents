import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { awaitingRelease, canDuplicate, duplicateRoutine, hasPendingEdit } from './routine-duplicate.ts'
import type { Routine } from './routine.ts'

const routine = { id: 'r1', botId: 'b1', name: 'Café', enabled: true } as Routine

test('o botão Duplicar aparece em rotina salva e some sem Hub ou em rotina nova', () => {
  assert.equal(canDuplicate(routine, false), true)
  assert.equal(canDuplicate(null, false), false)
  assert.equal(canDuplicate(routine, true), false)
})

test('duplicar envia só o bot e o id e devolve a cópia do Hub', async () => {
  const calls: Array<[string, Record<string, unknown>]> = []
  const copy = { id: 'r2', botId: 'b1', name: 'Café (cópia)', enabled: false } as Routine
  const client = { call: async <T>(method: string, params: Record<string, unknown>) => (calls.push([method, params]), { routine: copy } as T) }
  assert.equal(await duplicateRoutine(client, 'b1', 'r1'), copy)
  assert.deepEqual(calls, [['duplicateRoutine', { botId: 'b1', id: 'r1' }]])
})

test('um erro do Hub (teto de 1000, rotina sumida) chega ao chamador', async () => {
  const client = { call: async () => { throw new Error('Limite de 1000 rotinas atingido. Apague uma rotina para duplicar.') } }
  await assert.rejects(duplicateRoutine(client as never, 'b1', 'r1'), /Limite de 1000/)
})

test('pausa comum e liberação pendente são estados diferentes', () => {
  assert.equal(awaitingRelease({ enabled: false, externalReleasePending: true }), true)
  assert.equal(awaitingRelease({ enabled: false, externalReleasePending: false }), false)
  assert.equal(awaitingRelease({ enabled: false }), false)
  assert.equal(awaitingRelease({ enabled: true, externalReleasePending: true }), false)
})

test('edição não salva bloqueia o Duplicar; sem edição, libera', () => {
  const saved = { ...routine, instruction: 'Passar café', timeoutSeconds: 600 } as Routine
  const same = { name: 'Café', instruction: 'Passar café', timeout: '600', scheduleKey: 'k' }
  assert.equal(hasPendingEdit(saved, same, 'k'), false)
  assert.equal(hasPendingEdit(saved, same, null), true)
  assert.equal(hasPendingEdit(saved, { ...same, name: 'Chá' }, 'k'), true)
  assert.equal(hasPendingEdit(saved, { ...same, instruction: 'x' }, 'k'), true)
  assert.equal(hasPendingEdit(saved, { ...same, timeout: '60' }, 'k'), true)
  assert.equal(hasPendingEdit(saved, { ...same, scheduleKey: 'k2' }, 'k'), true)
  assert.equal(hasPendingEdit({ ...saved, timeoutSeconds: null } as Routine, { ...same, timeout: '3600' }, 'k'), false)
})

test('lista e calendário distinguem liberação pendente e os textos existem nos seis idiomas', () => {
  const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
  assert.match(read('../rotinas-central/RoutineList.tsx'), /awaitingRelease\(routine\)/)
  assert.match(read('../rotinas-central/parts.tsx'), /awaitingRelease\(item\.routine\)/)
  assert.match(read('../rotinas-central/parts.tsx'), /awaitingRelease\(routine\)/)
  assert.match(read('./RoutinesView.tsx'), /awaitingRelease\(routine\)/)
  for (const lang of ['pt-BR', 'en', 'es', 'fr', 'ar-AE', 'zh-CN']) {
    const catalog = read(`../../lib/fork-i18n/${lang}/rotinas.ts`)
    assert.match(catalog, /"rotinas\.editor\.duplicar":/, lang)
    assert.match(catalog, /"rotinas\.editor\.duplicando":/, lang)
    assert.match(catalog, /"rotinas\.editor\.duplicarSalveAntes":/, lang)
    assert.match(catalog, /"rotinas\.central\.aguardaLiberacao":/, lang)
  }
})
