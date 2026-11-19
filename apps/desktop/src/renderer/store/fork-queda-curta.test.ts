import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { test } from 'node:test'
import type { SSHState, SSHStatus } from '../../shared/ssh.ts'

// MX-217: a queda curta do link não fecha a tela; só a falha definitiva ou o fim do prazo soltam a loja.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      const url = new URL(`${specifier}.ts`, context.parentURL)
      if (existsSync(url)) return nextResolve(url.href, context)
    }
    return nextResolve(specifier, context)
  },
})

const load = async () => ({ ...(await import('./fork-queda-curta.ts')), ...(await import('../views/settings/fork-ssh-anexos.ts')) })

/** Relógio manual: `passar(ms)` dispara o que venceu. */
function relogio() {
  let agora = 1_000
  const timers = new Map<number, { quando: number; fn: () => void }>()
  let seq = 0
  return {
    agora: () => agora,
    depois: (fn: () => void, ms: number) => { const id = ++seq; timers.set(id, { quando: agora + ms, fn }); return id },
    cancelar: (id: unknown) => void timers.delete(id as number),
    passar(ms: number) {
      agora += ms
      for (const [id, t] of [...timers]) if (t.quando <= agora) { timers.delete(id); t.fn() }
    },
    pendentes: () => timers.size,
  }
}

const transporte = (nome: string) => ({ nome, call: async () => nome, stream: () => () => {} })
const PARADO = transporte('parado')
const CAIDO = () => Object.assign(new Error('caído'), { status: 0, isTransient: false }) as never

function lojaFalsa() {
  const log: string[] = []
  return { log, quedaDesde: null as number | null, restartStream() { log.push('restart') }, marcarQueda(d: number | null) { this.quedaDesde = d; log.push(`queda:${d}`) } }
}

test('LinkTrocavel: link caído usa o transporte parado, nunca o caminho antigo', async () => {
  const { LinkTrocavel } = await load()
  const link = new LinkTrocavel<string>('porta-1', (p) => transporte(p) as never, PARADO as never, CAIDO)
  assert.equal(await link.transporte().call('m', {}, 1), 'porta-1')
  link.trocar(null)
  assert.equal(link.vivo, false)
  assert.equal(link.transporte() as unknown, PARADO)
  link.trocar('porta-2')
  assert.equal(await link.transporte().call('m', {}, 1), 'porta-2')
})

test('LinkTrocavel: transporte de geração antiga falha fechado, inclusive no meio da chamada e no stream', async () => {
  const { LinkTrocavel } = await load()
  let soltar!: (v: unknown) => void
  const linhas: string[] = []
  let fim: unknown = 'aberto'
  let emitir!: (l: string) => void, terminar!: (e: unknown) => void
  const real = (nome: string) => ({ nome, call: () => new Promise((r) => { soltar = r }), stream: (_p: string, d: (l: string) => void, e: (x: unknown) => void) => { emitir = d; terminar = e; return () => {} } })
  const link = new LinkTrocavel<string>('t1', real as never, PARADO as never, CAIDO)
  const antigo = link.transporte()
  const pendente = antigo.call('m', {}, 1)
  antigo.stream('/events', (l) => linhas.push(l), (e) => { fim = e })
  emitir('a')
  link.trocar('t2')
  soltar('resposta do túnel antigo')
  await assert.rejects(pendente, /caído/)
  await assert.rejects(antigo.call('m', {}, 1), /caído/)
  emitir('b')
  terminar(null)
  assert.deepEqual(linhas, ['a'])
  assert.match(String((fim as Error).message), /caído/)
})

test('LojaSegura: cai, segura dentro do prazo, volta pelo caminho novo sem soltar', async () => {
  const { LinkTrocavel, LojaSegura, QUEDA_TOLERADA_MS } = await load()
  const r = relogio(), loja = lojaFalsa()
  const link = new LinkTrocavel<string>('c1', (p) => transporte(p) as never, PARADO as never, CAIDO)
  const segura = new LojaSegura(loja, link, r)
  let soltou = 0
  segura.cair(() => soltou++)
  assert.equal(segura.caida, true)
  assert.equal(link.vivo, false)
  assert.equal(loja.quedaDesde, 1_000)
  segura.cair(() => soltou++) // segunda queda não reinicia o prazo
  assert.equal(r.pendentes(), 1)
  r.passar(QUEDA_TOLERADA_MS - 1)
  assert.equal(soltou, 0)
  segura.voltar('c2')
  assert.equal(segura.caida, false)
  assert.equal(loja.quedaDesde, null)
  assert.equal(await link.transporte().call('m', {}, 1), 'c2')
  assert.equal(r.pendentes(), 0)
  r.passar(QUEDA_TOLERADA_MS)
  assert.equal(soltou, 0)
  assert.deepEqual(loja.log, ['restart', 'queda:1000', 'queda:null', 'restart'])
})

test('LojaSegura: o fim do prazo solta (tela de desconectado só depois de ~5 min)', async () => {
  const { LinkTrocavel, LojaSegura, QUEDA_TOLERADA_MS } = await load()
  assert.equal(QUEDA_TOLERADA_MS, 5 * 60_000)
  const r = relogio(), loja = lojaFalsa()
  const segura = new LojaSegura(loja, new LinkTrocavel<string>('c1', (p) => transporte(p) as never, PARADO as never, CAIDO), r)
  let soltou = 0
  segura.cair(() => soltou++)
  r.passar(QUEDA_TOLERADA_MS)
  assert.equal(soltou, 1)
  assert.equal(segura.caida, false)
})

test('regras: quedas temporárias e definitivas (SSH e nuvem), e quando a faixa aparece', async () => {
  const { sshQuedaTemporaria, nuvemQuedaTemporaria, emQueda } = await load()
  for (const kind of ['connecting', 'retrying', 'connected']) assert.equal(sshQuedaTemporaria({ kind }), true, kind)
  for (const kind of ['idle', 'failed', 'notInstalled', 'confirmHostKey']) assert.equal(sshQuedaTemporaria({ kind }), false, kind)
  assert.equal(sshQuedaTemporaria(undefined), false)
  for (const kind of ['connecting', 'reconnecting']) assert.equal(nuvemQuedaTemporaria({ kind, computerId: 'H' }, 'H'), true, kind)
  for (const kind of ['hubOffline', 'accessEnded', 'revocationPending', 'signedOut', 'handshakeRefused', 'hubUnverified', 'accountMismatch', 'ready', 'error'])
    assert.equal(nuvemQuedaTemporaria({ kind, computerId: 'H' }, 'H'), false, kind)
  assert.equal(nuvemQuedaTemporaria({ kind: 'reconnecting', computerId: 'OUTRO' }, 'H'), false)
  assert.equal(nuvemQuedaTemporaria({ kind: 'reconnecting' }, 'H'), false)
  assert.equal(nuvemQuedaTemporaria(null, 'H'), false)
  const s = (kind: string, linkCaidoDesde: number | null = null, mismatch: unknown = null) => ({ connection: { kind }, linkCaidoDesde, mismatch })
  assert.equal(emQueda(s('online')), false)
  assert.equal(emQueda(s('connecting')), false)
  assert.equal(emQueda(s('connecting', 5)), true)
  assert.equal(emQueda(s('offline')), true)
  assert.equal(emQueda(s('computerOffline')), true)
  assert.equal(emQueda(s('unauthorized', 5)), false)
  assert.equal(emQueda(s('offline', 5, { kind: 'updateApp' })), false)
})

const ID = 'hub-1', PERFIL = 'p1'
const perfil = { id: PERFIL, host: 'hub', port: null, user: null, identityFile: null, remotePort: 19422, computerId: ID, name: 'Hub' }
const anexo = (n: number) => ({ profileId: PERFIL, computer: { id: ID, name: 'Hub', signKey: 'k', boxKey: null }, baseURL: `http://127.0.0.1:${40000 + n}`, token: `t${n}` })
const estado = (status: SSHStatus | null, ...anexos: ReturnType<typeof anexo>[]): SSHState =>
  ({ profiles: [perfil], status: status ? { [PERFIL]: status } : {}, attachments: anexos })

function appFalso() {
  const stores = new Map<string, ReturnType<typeof lojaFalsa> & { transporte: () => unknown }>()
  const eventos: string[] = []
  return {
    stores, eventos,
    alvo: {
      store: (id: string) => stores.get(id) ?? null,
      computers: () => [],
      attach: (c: { id: string }, s: ReturnType<typeof lojaFalsa> & { transporte: () => unknown }) => { eventos.push(`attach:${c.id}`); stores.set(c.id, s) },
      detach: (id: string) => { eventos.push(`detach:${id}`); stores.delete(id) },
      lojasDo: (id: string) => (stores.get(id) ? [stores.get(id)!] : []),
    },
  }
}

async function anexosSSH() {
  const { AnexosSSH } = await load()
  const app = appFalso(), r = relogio()
  const anexos = new AnexosSSH(app.alvo, (_c, transporte) => ({ ...lojaFalsa(), transporte }), (t) => transporte(t.baseURL) as never, PARADO as never, CAIDO, r)
  const nome = () => (app.stores.get(ID)!.transporte() as { call: (m: string, b: unknown, t: number) => Promise<string> }).call('m', {}, 1)
  return { anexos, app, r, nome }
}

test('SSH: túnel cai e volta em outra porta: a mesma loja segue, sem detach (a tela e a seleção ficam)', async () => {
  const { anexos, app, nome } = await anexosSSH()
  anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
  const loja = app.stores.get(ID)!
  assert.equal(await nome(), 'http://127.0.0.1:40001')
  anexos.sync(estado({ kind: 'retrying', message: 'ssh tunnel ended' }))
  assert.equal(app.stores.get(ID), loja)
  assert.equal(anexos.segurando(ID), true)
  assert.notEqual(loja.quedaDesde, null)
  assert.equal(await nome(), 'parado')
  anexos.sync(estado({ kind: 'connecting', step: 'ssh' }))
  anexos.sync(estado({ kind: 'connected', localPort: 40002 }, anexo(2)))
  assert.equal(app.stores.get(ID), loja)
  assert.equal(anexos.segurando(ID), false)
  assert.equal(loja.quedaDesde, null)
  assert.equal(await nome(), 'http://127.0.0.1:40002')
  assert.deepEqual(app.eventos, [`attach:${ID}`])
})

test('SSH: queda longa solta depois do prazo; falha definitiva, desligar ou remover soltam na hora', async () => {
  const { QUEDA_TOLERADA_MS } = await load()
  {
    const { anexos, app, r } = await anexosSSH()
    anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
    anexos.sync(estado({ kind: 'retrying', message: 'x' }))
    r.passar(QUEDA_TOLERADA_MS - 1)
    assert.ok(app.stores.has(ID))
    r.passar(1)
    assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`])
    // Volta depois do prazo: loja nova (do cache), como antes.
    anexos.sync(estado({ kind: 'connected', localPort: 40002 }, anexo(2)))
    assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`, `attach:${ID}`])
  }
  for (const definitiva of [{ kind: 'failed', message: 'Permission denied' }, { kind: 'idle' }, { kind: 'confirmHostKey', name: 'h', fingerprints: [] }] as SSHStatus[]) {
    const { anexos, app, r } = await anexosSSH()
    anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
    anexos.sync(estado(definitiva))
    assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`], definitiva.kind)
    assert.equal(r.pendentes(), 0)
  }
  {
    // Em queda, depois falha definitiva: solta na hora, sem esperar o prazo.
    const { anexos, app, r } = await anexosSSH()
    anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
    anexos.sync(estado({ kind: 'retrying', message: 'x' }))
    anexos.sync(estado({ kind: 'failed', message: 'Host key changed' }))
    assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`])
    assert.equal(r.pendentes(), 0)
  }
  {
    // Perfil removido durante a queda.
    const { anexos, app } = await anexosSSH()
    anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
    anexos.sync({ profiles: [], status: {}, attachments: [] })
    assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`])
  }
})

test('SSH: outro computador atrás do mesmo perfil, ou loja trocada por outro dono, não é retomado', async () => {
  const { anexos, app } = await anexosSSH()
  anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
  const primeira = app.stores.get(ID)
  anexos.sync(estado({ kind: 'retrying', message: 'x' }))
  const outraChave = { ...anexo(2), computer: { ...anexo(2).computer, signKey: 'outra' } }
  anexos.sync(estado({ kind: 'connected', localPort: 40002 }, outraChave))
  assert.notEqual(app.stores.get(ID), primeira)
  assert.deepEqual(app.eventos, [`attach:${ID}`, `attach:${ID}`])
  // Outra loja tomou o lugar (host local): a queda não a solta.
  app.stores.set(ID, { ...lojaFalsa(), transporte: () => null })
  anexos.sync(estado({ kind: 'retrying', message: 'x' }))
  assert.ok(app.stores.has(ID))
  anexos.parar()
  assert.ok(app.stores.has(ID))
})

test('SSH: desligar à mão, sem passar pela queda, revoga o transporte já entregue antes do detach', async () => {
  const { anexos, app } = await anexosSSH()
  anexos.sync(estado({ kind: 'connected', localPort: 40001 }, anexo(1)))
  const capturado = app.stores.get(ID)!.transporte() as { call: (m: string, b: unknown, t: number) => Promise<string> }
  assert.equal(await capturado.call('m', {}, 1), 'http://127.0.0.1:40001')
  anexos.sync(estado({ kind: 'idle' }))
  assert.deepEqual(app.eventos, [`attach:${ID}`, `detach:${ID}`])
  await assert.rejects(capturado.call('m', {}, 1), /caído/)
})
