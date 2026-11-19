import test from 'node:test'
import assert from 'node:assert/strict'
import { ManagedPoll, POLL, type PollClock, type PollOutcome } from './managed-poll.ts'

const HOUR = 3_600_000

/** Relógio falso: timers por ordem de vencimento; cada leitura falsa é síncrona. */
function fakeClock(random = 0.5) {
  let now = 0, seq = 0
  const timers = new Map<number, { at: number; fn: () => void }>()
  const clock: PollClock = {
    now: () => now,
    setTimeout: (fn, ms) => { const id = ++seq; timers.set(id, { at: now + Math.max(0, ms), fn }); return id },
    clearTimeout: (id) => { timers.delete(id as number) },
    random: () => random,
  }
  const flush = async () => { for (let i = 0; i < 20; i++) await Promise.resolve() }
  async function advance(ms: number) {
    const end = now + ms
    await flush()
    for (;;) {
      let next: [number, { at: number; fn: () => void }] | null = null
      for (const entry of timers) if (entry[1].at <= end && (!next || entry[1].at < next[1].at)) next = entry
      if (!next) break
      timers.delete(next[0]); now = next[1].at; next[1].fn(); await flush()
    }
    now = end
  }
  return { clock, advance, flush, get now() { return now } }
}

/** Servidor falso com o custo de rede de advanceManaged: health + managed-offer por leitura. */
function fakeServer() {
  const s = { signedIn: true, network: true, flow: false, revoked: false, requests: 0, at: [] as number[], signOuts: 0 }
  const tick = (now: () => number) => async (): Promise<PollOutcome> => {
    if (!s.signedIn) return 'idle' // sem principal: nenhum pedido (service.pollManaged)
    s.requests++; s.at.push(now()) // health
    if (!s.network) return 'error'
    if (s.revoked) { s.signedIn = false; s.signOuts++; return 'idle' } // token recusado → conta fechada (fork-account)
    s.requests++; s.at.push(now()) // managed-offer
    return s.flow ? 'active' : 'idle'
  }
  return { s, tick }
}

/** Antes (service.ts:510 em origin/main): setInterval de 2 s, sempre, em qualquer situação. */
async function before(setup: (s: ReturnType<typeof fakeServer>['s']) => void) {
  const c = fakeClock(), { s, tick } = fakeServer(); setup(s)
  const read = tick(() => c.now)
  const every = () => { c.clock.setTimeout(every, 2_000); void read() }
  c.clock.setTimeout(every, 2_000)
  await c.advance(HOUR)
  return s.requests
}
async function after(setup: (s: ReturnType<typeof fakeServer>['s']) => void, opts: { hidden?: boolean; away?: boolean } = {}) {
  const c = fakeClock(), { s, tick } = fakeServer(); setup(s)
  const poll = new ManagedPoll(tick(() => c.now), () => !!opts.away, c.clock)
  if (opts.hidden) poll.pause(); else poll.now()
  await c.advance(HOUR)
  poll.stop()
  return s.requests
}

test('harness antes/depois: pedidos por hora em cada situação', async () => {
  const rows: [string, number, number][] = [
    ['janela visível, sem oferta (uso normal)', await before(() => {}), await after(() => {})],
    ['janela escondida/minimizada', await before(() => {}), await after(() => {}, { hidden: true })],
    ['computador ocioso (≥5 min sem uso)', await before(() => {}), await after(() => {}, { away: true })],
    ['sem rede / erro', await before(s => { s.network = false }), await after(s => { s.network = false })],
    ['fluxo gerenciado ativo (vínculo/bootstrap)', await before(s => { s.flow = true }), await after(s => { s.flow = true })],
    ['fora da conta', await before(s => { s.signedIn = false }), await after(s => { s.signedIn = false })],
  ]
  console.log('\n| situação | antes (pedidos/h) | depois (pedidos/h) |\n|---|---:|---:|\n' + rows.map(r => `| ${r[0]} | ${r[1]} | ${r[2]} |`).join('\n'))
  assert.equal(rows[0][1], 3600)
  assert.ok(rows[0][2] <= 40, `visível ocioso: ${rows[0][2]}`)
  assert.equal(rows[1][2], 0); assert.equal(rows[2][2], 0); assert.equal(rows[5][2], 0)
  assert.ok(rows[3][2] <= 20)
  assert.ok(rows[4][2] >= rows[4][1]) // ritmo curto preservado enquanto o fluxo existe (+1 leitura imediata)
})

test('janela escondida → zero consultas periódicas; ao voltar, uma consulta imediata', async () => {
  const c = fakeClock(), { s, tick } = fakeServer()
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now(); await c.flush()
  assert.equal(s.requests, 2)
  poll.pause()
  await c.advance(HOUR)
  assert.equal(s.requests, 2)
  assert.equal(poll.resume(), true); await c.flush()
  assert.equal(s.requests, 4)
  assert.equal(c.now, HOUR) // na hora, sem esperar timer
  assert.equal(poll.resume(), false) // já à vista: sem leitura extra
  await c.flush(); assert.equal(s.requests, 4)
  poll.stop()
})

test('erro repetido → intervalo cresce até o teto, com aleatoriedade; sucesso não volta ao curto sozinho', async () => {
  const c = fakeClock(0), { s, tick } = fakeServer(); s.network = false
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now()
  await c.advance(2 * HOUR)
  const gaps = s.at.slice(1).map((t, i) => t - s.at[i])
  assert.deepEqual(gaps.slice(0, 5), [60_000, 120_000, 240_000, 300_000, 300_000])
  assert.ok(gaps.every(g => g <= POLL.maxMs))
  poll.stop()
  const j = fakeClock(1), other = fakeServer(); other.s.network = false
  const jittered = new ManagedPoll(other.tick(() => j.now), () => false, j.clock)
  jittered.now(); await j.advance(POLL.maxMs)
  assert.equal(other.s.at[1], POLL.baseMs * (1 - POLL.jitter))
  jittered.stop()
})

test('sessão revogada → detectada no próximo pedido: na volta do foco e, visível, em até um intervalo', async () => {
  const c = fakeClock(), { s, tick } = fakeServer()
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now(); await c.advance(10 * 60_000) // já no intervalo mais longo
  s.revoked = true
  const t0 = c.now
  await c.advance(POLL.maxMs)
  assert.equal(s.signOuts, 1, 'visível: detectado em até um intervalo longo')
  assert.ok(s.at.at(-1)! - t0 <= POLL.maxMs)
  const n = s.requests
  await c.advance(HOUR)
  assert.equal(s.requests, n, 'conta fechada: sem pedidos')
  poll.stop()

  const f = fakeClock(), other = fakeServer()
  const focused = new ManagedPoll(other.tick(() => f.now), () => false, f.clock)
  focused.now(); await f.advance(30_000)
  focused.pause(); await f.advance(HOUR)
  other.s.revoked = true
  focused.resume(); await f.flush()
  assert.equal(other.s.signOuts, 1, 'volta do foco: pedido imediato detecta')
  focused.stop()
})

test('sair da conta → nenhum pedido a partir da hora', async () => {
  const c = fakeClock(), { s, tick } = fakeServer(); s.flow = true
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now(); await c.advance(10_000)
  s.signedIn = false; poll.now() // service.ts: evento 'epoch' chama now(); sem sessão, sem rede
  const n = s.requests
  await c.advance(HOUR)
  assert.equal(s.requests, n)
  poll.stop()
})

test('fluxo de vínculo ativo → ritmo curto só nele; depois volta à espera crescente', async () => {
  const c = fakeClock(), { s, tick } = fakeServer(); s.flow = true
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now(); await c.advance(20_000)
  const gaps = s.at.filter((_, i) => i % 2 === 0).slice(1).map((t, i, a) => t - (i ? a[i - 1] : 0))
  assert.ok(gaps.every(g => g === POLL.activeMs))
  s.flow = false
  await c.advance(POLL.activeMs)
  const ended = c.now, n = s.requests
  await c.advance(POLL.baseMs * 0.9 - 1)
  assert.equal(s.requests, n, 'fora do fluxo: nada antes do intervalo base')
  await c.advance(1)
  assert.equal(s.requests, n + 2)
  assert.ok(c.now - ended < POLL.baseMs)
  poll.stop()
})

test('ação do usuário → consulta logo (respeitando o mínimo de 2 s), sem dobrar leituras', async () => {
  const c = fakeClock(), { s, tick } = fakeServer()
  const poll = new ManagedPoll(tick(() => c.now), () => false, c.clock)
  poll.now(); await c.advance(30_000)
  assert.equal(s.requests, 2)
  poll.now(); await c.flush()
  assert.equal(s.requests, 4)
  poll.now(); await c.flush() // logo depois: espera o mínimo
  assert.equal(s.requests, 4)
  await c.advance(POLL.activeMs)
  assert.equal(s.requests, 6)
  poll.stop()
})

test('computador ocioso → só checagem local; ao voltar o uso, consulta em até 1 min', async () => {
  const c = fakeClock(), { s, tick } = fakeServer()
  let away = true
  const poll = new ManagedPoll(tick(() => c.now), () => away, c.clock)
  poll.now(); await c.advance(HOUR)
  assert.equal(s.requests, 0)
  away = false
  await c.advance(POLL.awayCheckMs)
  assert.equal(s.requests, 2)
  poll.stop()
})
