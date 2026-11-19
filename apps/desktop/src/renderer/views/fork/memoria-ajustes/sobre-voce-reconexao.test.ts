import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../../../shared/models'
import type { HostTransport } from '../../../client/host-client'
import { SobreVoceStore, identidade } from './sobre-voce.ts'

// MX-8 (revisão Codex A): o hello real de uma reconexão ao MESMO Hub sobe a época (1 → 2), mas não muda a
// identidade do cartão "Sobre você"; então a tela não recria o store e o rascunho fica. BotStore e HostClient são
// reais; só o transporte, a ponte do Electron e o relógio são fictícios (como em computador/screen-epoch.test.ts).

const bundle = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'`, resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})

function clock() {
  let now = 1_000_000
  let seq = 0
  const timers = new Map<number, { at: number; fn: () => void }>()
  class FakeDate extends Date { static override now() { return now } }
  return {
    Date: FakeDate,
    setTimeout: (fn: () => void, ms = 0) => { timers.set(++seq, { at: now + ms, fn }); return seq },
    clearTimeout: (id: number) => { timers.delete(id) },
    async advance(ms: number) {
      const end = now + ms
      for (;;) {
        const next = [...timers.entries()].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
        if (!next) break
        timers.delete(next[0])
        now = next[1].at
        next[1].fn()
        await settle()
      }
      now = end
      await settle()
    },
  }
}
const settle = async () => { for (let i = 0; i < 8; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })

test('editar → cair → hello novo do mesmo Hub (época 1 → 2): mesma identidade, rascunho fica', async () => {
  const time = clock()
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date: time.Date, setTimeout: time.setTimeout, clearTimeout: time.clearTimeout,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const { BotStore } = module.exports as { BotStore: typeof import('../../../store/bot-store').BotStore }
  let content: string | null = '## Quem é\nAna'
  let end!: Parameters<HostTransport['stream']>[2]
  const transport: HostTransport = {
    async call(method, body) {
      if (method === 'hello') return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
      if (method === 'teamSettings') return { primaryBotId: 'donna' }
      if (method === 'memoryProfile') return { content, revision: '1', loose: 0 }
      if (method === 'saveMemoryProfile') { content = (body as { content: string }).content; return { content, revision: '2', loose: 0 } }
      return {}
    },
    stream(_path, onData, onEnd) {
      end = onEnd
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 9, usage: { providers: [] }, screen: null })))
      return () => {}
    },
  }
  const host = new BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => transport)
  host.start()
  try {
    await settle()
    assert.ok(host.connectedClient, 'conectado')
    const epoca = host.principalEpoch
    const chave = identidade(host)
    const store = new SobreVoceStore(async () => {
      const c = host.connectedClient
      if (!c) throw new Error('memory_connection_unavailable')
      return c
    })
    await store.load()
    store.editar('fala', 'Curto')

    end(Object.assign(new Error('stream'), { status: 0 }) as never)
    await settle()
    await store.load()
    assert.equal(store.rascunho.fala, 'Curto', 'sem conexão o rascunho fica')

    await time.advance(6000)
    assert.ok(host.connectedClient, 'reconectou')
    assert.ok(host.principalEpoch > epoca, `a época subiu (${epoca} → ${host.principalEpoch})`)
    assert.equal(identidade(host), chave, 'a identidade do cartão não mudou: a tela não recria o store')
    await store.load()
    assert.equal(store.problema, null)
    assert.equal(store.rascunho.fala, 'Curto', 'o hello novo não apaga o rascunho')
    await store.save()
    assert.match(content!, /## Como fala\nCurto/)
  } finally { host.retire() }
})
