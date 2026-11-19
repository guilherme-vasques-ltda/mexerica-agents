import assert from 'node:assert/strict'
import test from 'node:test'
import { OLHO_MS } from '../../../../shared/fork-cofre-seguro.ts'
import { Olho } from './olho.ts'

// MX-154 (decisão do dono): o olho mostra o valor por no máximo 10 s; esconde ao sair do campo e ao fechar.
function relogio() {
  let agora = 0
  const tarefas = new Map<number, { em: number; fn: () => void }>()
  let n = 0
  return {
    agendar: (fn: () => void, ms: number) => { tarefas.set(++n, { em: agora + ms, fn }); return n },
    cancelar: (id: unknown) => { tarefas.delete(id as number) },
    avancar(ms: number) {
      agora += ms
      for (const [id, t] of [...tarefas]) if (t.em <= agora) { tarefas.delete(id); t.fn() }
    },
    pendentes: () => tarefas.size,
  }
}
const criar = () => {
  const r = relogio()
  const eventos: boolean[] = []
  const olho = new Olho({ aoMudar: (v) => eventos.push(v), agendar: r.agendar, cancelar: r.cancelar })
  return { r, eventos, olho }
}

test('o limite é de 10 segundos', () => assert.equal(OLHO_MS, 10_000))

test('mostrar: visível e some sozinho depois de 10 s, nunca antes', () => {
  const { r, eventos, olho } = criar()
  olho.mostrar()
  assert.equal(olho.visivel, true)
  r.avancar(OLHO_MS - 1)
  assert.equal(olho.visivel, true)
  r.avancar(1)
  assert.equal(olho.visivel, false)
  assert.deepEqual(eventos, [true, false])
  assert.equal(r.pendentes(), 0)
})

test('sair do campo (blur) ou fechar esconde na hora e cancela o relógio', () => {
  const { r, eventos, olho } = criar()
  olho.mostrar()
  r.avancar(3000)
  olho.esconder()
  assert.equal(olho.visivel, false)
  assert.equal(r.pendentes(), 0)
  r.avancar(OLHO_MS)
  assert.deepEqual(eventos, [true, false])
})

test('mostrar de novo reinicia os 10 s; o olho alterna; esconder sem estar visível não faz nada', () => {
  const { r, eventos, olho } = criar()
  olho.esconder()
  assert.deepEqual(eventos, [])
  olho.mostrar()
  r.avancar(8000)
  olho.mostrar()
  r.avancar(8000)
  assert.equal(olho.visivel, true)
  r.avancar(2000)
  assert.equal(olho.visivel, false)
  olho.alternar()
  assert.equal(olho.visivel, true)
  olho.alternar()
  assert.equal(olho.visivel, false)
  assert.equal(r.pendentes(), 0)
})
