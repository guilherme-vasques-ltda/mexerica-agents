import assert from 'node:assert/strict'
import test from 'node:test'
import { LIMITES, LimitesChamada, type EstadoLimites } from './limites-chamada.ts'

const MIN = 60_000

function relogio() {
  let agora = 1_000_000
  const mudancas: EstadoLimites[] = []
  const limites = new LimitesChamada(() => agora, (e) => mudancas.push(e))
  return {
    limites, mudancas,
    passar(ms: number) { agora += ms; return limites.conferir() },
  }
}

test('5 minutos sem conversa: avisa aos 4min30 com Continuar e encerra aos 5', () => {
  const r = relogio()
  assert.deepEqual(r.passar(4 * MIN + 29_000), { aviso: null, fim: null })
  assert.deepEqual(r.passar(1_000), { aviso: 'silencio', fim: null })
  r.limites.continuar()
  assert.deepEqual(r.limites.atual, { aviso: null, fim: null }, 'Continuar zera o relógio de silêncio')
  assert.deepEqual(r.passar(4 * MIN + 30_000), { aviso: 'silencio', fim: null })
  assert.deepEqual(r.passar(30_000), { aviso: null, fim: 'silencio' })
})

test('fala real da pessoa adia o fim; bot falando ou trabalhando suspende o silêncio, nunca o teto', () => {
  const r = relogio()
  r.passar(4 * MIN)
  r.limites.conversou()
  assert.equal(r.passar(4 * MIN).fim, null, 'conversa recente')
  r.limites.botOcupado(true)
  assert.equal(r.passar(10 * MIN).fim, null, 'o bot trabalhando não conta como silêncio')
  r.limites.botOcupado(false)
  assert.equal(r.passar(4 * MIN).aviso, null, 'o silêncio recomeça quando o bot termina')
  r.limites.botOcupado(true)
  // Já se passaram 22 min; o teto corre mesmo com o bot ocupado.
  assert.deepEqual(r.passar(7 * MIN), { aviso: 'teto', fim: null })
  assert.deepEqual(r.passar(MIN), { aviso: null, fim: 'teto' })
})

test('teto de 30 minutos com aviso aos 29, sem reiniciar por conversa', () => {
  const r = relogio()
  for (let i = 0; i < 28; i++) { r.passar(MIN); r.limites.conversou() }
  assert.deepEqual(r.limites.atual, { aviso: null, fim: null })
  r.passar(MIN)
  r.limites.conversou()
  assert.deepEqual(r.limites.atual, { aviso: 'teto', fim: null })
  assert.deepEqual(r.passar(MIN), { aviso: null, fim: 'teto' })
})

test('prazo decorrido: um tick atrasado (janela oculta, Mac suspenso) encerra na hora, e o fim é idempotente', () => {
  const r = relogio()
  assert.equal(r.limites.proximoPrazo(), LIMITES.avisoSilencio)
  assert.deepEqual(r.passar(2 * 60 * MIN), { aviso: null, fim: 'teto' }, 'sem avisos perdidos no meio')
  const antes = r.mudancas.length
  r.limites.conversou()
  r.limites.continuar()
  r.limites.botOcupado(true)
  assert.deepEqual(r.passar(MIN), { aviso: null, fim: 'teto' })
  assert.equal(r.mudancas.length, antes, 'depois do fim, nada muda nem avisa de novo')
  assert.equal(r.limites.proximoPrazo(), null)
})

test('o próximo prazo nunca passa do próximo limiar', () => {
  const r = relogio()
  r.passar(4 * MIN)
  assert.equal(r.limites.proximoPrazo(), 30_000)
  r.limites.botOcupado(true)
  assert.equal(r.limites.proximoPrazo(), LIMITES.avisoTeto - 4 * MIN, 'com o bot ocupado, só o teto agenda')
})

test('o relógio se reagenda quando o bot termina: silêncio volta a encerrar sem depender de foco (achado Codex P2)', () => {
  let agora = 0
  const timers: { em: number; fn: () => void; id: number }[] = []
  let ids = 0
  const relogio = {
    set: (fn: () => void, ms: number) => { const id = ++ids; timers.push({ em: agora + ms, fn, id }); return id },
    clear: (id: unknown) => { const i = timers.findIndex((t) => t.id === id); if (i >= 0) timers.splice(i, 1) },
  }
  const avancar = (ms: number) => {
    const alvo = agora + ms
    for (;;) {
      timers.sort((a, b) => a.em - b.em)
      const t = timers[0]
      if (!t || t.em > alvo) break
      timers.shift()
      agora = t.em
      t.fn()
    }
    agora = alvo
  }
  const limites = new LimitesChamada(() => agora, () => {})
  const parar = limites.iniciarRelogio(relogio)
  avancar(4 * MIN)
  limites.botOcupado(true)
  avancar(MIN)
  limites.botOcupado(false) // aos 5 min: o silêncio recomeça daqui
  avancar(4 * MIN + 29_000)
  assert.deepEqual(limites.atual, { aviso: null, fim: null })
  avancar(1_000)
  assert.deepEqual(limites.atual, { aviso: 'silencio', fim: null }, 'aviso aos 9min30, só pelo timer')
  avancar(30_000)
  assert.deepEqual(limites.atual, { aviso: null, fim: 'silencio' }, 'fim aos 10 min, só pelo timer')
  assert.equal(timers.length, 0, 'depois do fim, nenhum timer fica')
  parar()
})
