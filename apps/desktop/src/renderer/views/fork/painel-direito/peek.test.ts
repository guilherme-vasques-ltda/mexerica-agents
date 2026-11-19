import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { PanelPeek } from './peek.ts'

function clock() {
  let now = 0
  const pending = new Map<number, { at: number; fn: () => void }>()
  let id = 0
  return {
    timers: { set: (fn: () => void, ms: number) => { pending.set(++id, { at: now + ms, fn }); return id }, clear: (k: unknown) => void pending.delete(k as number) },
    advance(ms: number) {
      now += ms
      for (const [k, t] of [...pending].sort((a, b) => a[1].at - b[1].at)) if (t.at <= now) { pending.delete(k); t.fn() }
    },
  }
}

test('painel solto: abre 90 ms depois da borda, fecha 280 ms depois de sair', () => {
  const c = clock()
  const seen: boolean[] = []
  const p = new PanelPeek((open) => seen.push(open), c.timers)
  p.edgeEnter()
  c.advance(89)
  assert.equal(p.open, false)
  c.advance(1)
  assert.equal(p.open, true)
  // Do traço para o painel: não fecha.
  p.edgeLeave()
  p.panelEnter()
  c.advance(1000)
  assert.equal(p.open, true)
  p.panelLeave()
  c.advance(279)
  assert.equal(p.open, true)
  c.advance(1)
  assert.equal(p.open, false)
  assert.deepEqual(seen, [true, false])
})

test('painel solto: passar rápido pela borda não abre; campo com foco segura; Esc fecha já', () => {
  const c = clock()
  const p = new PanelPeek(() => {}, c.timers)
  p.edgeEnter()
  c.advance(40)
  p.edgeLeave()
  c.advance(500)
  assert.equal(p.open, false)
  p.show()
  p.panelLeave(true)
  c.advance(1000)
  assert.equal(p.open, true)
  p.dismiss()
  assert.equal(p.open, false)
})

// Dono, 07/11: solto, o painel fica sobre a faixa de arrasto do chat; sem no-drag no próprio painel, no macOS a
// engrenagem e o alfinete viram arrasto da janela e o painel fecha. Prova no navegador: docs/reference/design/painel-abas-20261107/arrasto.mjs.
test('o painel inteiro fica fora da região de arrasto da janela', () => {
  const css = readFileSync(new URL('./painel-direito.css', import.meta.url), 'utf8')
  const rule = /(?:^|\n)\.rpanel \{([^}]*)\}/.exec(css)?.[1] ?? ''
  assert.match(rule, /-webkit-app-region:\s*no-drag/)
})
