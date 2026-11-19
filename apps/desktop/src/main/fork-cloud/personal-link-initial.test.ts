import assert from 'node:assert/strict'
import test from 'node:test'
import * as path from 'node:path'
import * as fs from 'node:fs'
import * as os from 'node:os'
import * as nodeCrypto from 'node:crypto'
import { PassThrough } from 'node:stream'
import { EventEmitter } from 'node:events'
import * as crypto from './crypto.ts'
import * as w from './personal-link-wire.ts'
import { loadMainModule, settle } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'

// D70 PR-A (Main): consumidor REAL com SSH, Hub e cofre simulados. Mock não qualifica SSH, cofre ou SO.
const anchor = { schema: 1, qualificationId: 'fixture', evidenceDigest: 'e'.repeat(64), ca: 'ssh-ed25519 AAAA', caFingerprint: 'SHA256:x', host: 'hub.example.invalid', port: 22, user: 'fixture', principal: 'hub.example.invalid', admittedUntil: 4_000_000_000_000, command: '/usr/bin/donnabot-host personal-link helper' }
const profile = { id: 'fixture', host: anchor.host, user: anchor.user, port: 22, remotePort: 19422, identityFile: '/synthetic/key' }
const id = (n: number) => crypto.b64url(crypto.random(n))

/** Hub D430 falso: mesmos objetos do v2; `lose` = commita e derruba a conexão antes da resposta. */
function fakeHub() {
  const H = crypto.signerFromSeed(crypto.newDeviceSeed()), G = id(32), R = id(32), rootId = id(16), admission = id(16)
  const s = { mutations: 0, frames: 0, lose: new Set<string>(), hold: null as null | (() => void), ra: null as w.Signed | null, rc: null as w.Signed | null, m: null as w.Signed | null, mr: null as w.Signed | null, ch: null as w.Signed | null, payload: null as w.J[] | null, children: [] as { killed: boolean }[], drop: new Set<string>(), ops: [] as string[], corrupt: new Map<string, 'id' | 'op' | 'shape'>(), holdOn: new Map<string, () => void>(), reopened: false, closed: [] as w.Signed[], reconcileWith: null as w.J | null, ac: null as w.Signed | null }
  const D = (): w.J => [w.D430.deadline, 1, 'b', R, 9, 30000]
  const handle = (op: string, a: w.J[]): w.J => {
    const T = a[0] as string
    switch (op) {
      case 'reconcile': if (s.reconcileWith) { const v = s.reconcileWith; s.reconcileWith = null; return v }
        if (s.reopened && s.ra && !s.rc) return ['closed', admission, s.ra]
        return !s.ra ? ['none'] : !s.rc ? ['pending', admission, s.ra] : !s.mr ? ['complete', admission, s.ra, s.rc] : ['managed', admission, s.ra, s.rc, s.m!, s.mr, w.chainDigestT(s.ra, s.rc, s.m!, s.mr)]
      case 'root-challenge': if (s.ra && !(s.reopened && !s.rc)) throw 0; s.ch = w.signT(H, [w.D430.challenge, 1, crypto.b64url(H.publicKey), G, a[1], R, id(16), T, a[2], id(32), D(), rootId, 1, w.P0]); return s.ch
      case 'root-accept': { const c = s.ch![0], ac = a[0] as w.Signed; if (s.reopened && s.ra && !s.rc) { s.closed.push(s.ra); s.ra = null; s.reopened = false }
        if (s.ra || ac[0][10] !== w.hashJ(s.ch!)) throw 0; s.mutations++; s.ac = ac; s.ra = w.signT(H, [w.D430.admission, 1, ...c.slice(2, 8), c[11], c[12], admission, w.hashJ(s.ch!), w.hashJ(ac), w.P0]); return s.ra }
      case 'persist-challenge': if (s.reopened) throw { code: 'reopened' }; if (s.rc) throw 0; return [id(32), D(), R]
      case 'persist-confirm': { const pc = a[0] as w.Signed, r = s.ra![0]; if (s.rc) throw 0; s.mutations++; s.rc = w.signT(H, [w.D430.complete, 1, r[2], r[3], r[4], R, admission, w.hashJ(s.ra!), w.hashJ(pc), w.P0]); return s.rc }
      case 'management-challenge': { const r = s.ra![0]; if (s.mr) throw 0; s.payload = [w.D430.management, 1, r[2], r[3], r[4], R, id(16), T, w.hashJ(s.ra!), 1, w.P1, id(32), D(), 1]; return s.payload }
      case 'management-accept': { const m = a[0] as w.Signed, r = s.ra![0]; if (s.mr) throw 0; s.mutations++; s.m = m; s.mr = w.signT(H, [w.D430.receipt, 1, r[2], r[3], r[4], R, m[0][6], r[7], r[8], w.hashJ(s.ra!), w.hashJ(m), w.P1]); return s.mr }
    }
    throw 0
  }
  const spawn = () => {
    const emitter = new EventEmitter()
    const child = Object.assign(emitter, { stdout: new PassThrough(), stdin: new PassThrough(), stderr: new PassThrough(), exitCode: null as number | null, killed: false, kill() { child.killed = true; emitter.emit('exit', null) } })
    child.stdin.on('data', (frame: Buffer) => {
      s.frames++
      const v = w.readJ(frame.subarray(4)) as w.J[], op = v[3] as string
      s.ops.push(op)
      const before = s.holdOn.get(op)
      if (before) { s.holdOn.delete(op); before() }
      if (s.drop.delete(op)) { child.kill(); return }
      const reply = () => {
        let out: w.J
        try { out = [w.D430.procedure, 1, v[2], 'ok', op, handle(op, v.slice(4))] } catch (e) { out = [w.D430.procedure, 1, v[2], 'error', (e as { code?: string } | null)?.code ?? 'refused'] }
        if (s.lose.delete(op)) { child.kill(); return }
        const bad = s.corrupt.get(op)
        // Resposta canônica, mas fora do contrato do pedido em voo (id, op ou forma).
        if (bad === 'id') out = [w.D430.procedure, 1, id(16), 'ok', op, ['none']]
        else if (bad === 'op') out = [w.D430.procedure, 1, v[2], 'ok', 'root-challenge', ['none']]
        else if (bad === 'shape') out = [w.D430.procedure, 1, v[2], 'ok', op]
        const bytes = w.j(out), head = Buffer.alloc(4); head.writeUInt32BE(bytes.length); child.stdout.write(Buffer.concat([head, bytes]))
      }
      if (s.hold) { const go = s.hold; s.hold = null; go(); setImmediate(reply) } else reply()
    })
    s.children.push(child)
    return child
  }
  /** RA assinada por H com UM campo trocado (outra operação/chave/contexto), para respostas `closed` adulteradas. */
  const raTrocada = (i: number, valor: w.J): w.Signed => {
    const p = [...s.ra![0]]
    p[i] = valor
    return w.signT(H, p)
  }
  return Object.assign(s, { spawn, raTrocada, admission })
}

function vaultMock() {
  const counters = { seals: 0, signs: 0 }, gens: Map<string, Buffer>[] = []
  const make = () => {
    const files = new Map<string, Buffer>(), signer = crypto.signerFromSeed(crypto.newDeviceSeed())
    gens.push(files)
    const counted = { publicKey: signer.publicKey, sign(m: Uint8Array) { counters.signs++; return signer.sign(m) } }
    const vault: Record<string, unknown> = {
      async read(n: string) { return files.get(n) ?? null },
      async seal(n: string, b: Uint8Array) { if (files.has(n)) throw new Error('exists'); counters.seals++; files.set(n, Buffer.from(b)); return Buffer.from(b) },
      async has(p: string) { return [...files.keys()].some(k => k.startsWith(`${p}.`)) },
      async signer() { return counted },
      async next() { return make() },
    }
    return vault
  }
  const vault = make()
  return { files: gens[0], gens, counters, vault }
}

function load(hub: ReturnType<typeof fakeHub>, vault: ReturnType<typeof vaultMock>['vault']) {
  const storeBytes = Buffer.from(`@cert-authority ${anchor.principal} ${anchor.ca}\n`)
  return loadMainModule<typeof import('./personal-link-initial.ts')>('fork-cloud/personal-link-initial', {
    'node:child_process': { spawn: hub.spawn }, 'node:crypto': nodeCrypto, 'node:path': path, './crypto': crypto, './personal-link-wire': w,
    'node:fs': { promises: { lstat: async () => ({ isFile: () => true, uid: 0, mode: 0o755 }), mkdtemp: async (p: string) => `${p}x`, chmod: async () => {}, writeFile: async () => {}, rm: async () => {} } },
    electron: { app: { getPath: () => '/synthetic' } },
    './personal-ssh': { protectedParents: async () => {}, certificate: () => ({ until: 4_000_000_000_000 }), privateFile: async (p: string) => ({ bytes: p.endsWith('ca') ? storeBytes : Buffer.from('log'), dev: 1, ino: 1 }) },
    './personal-link-trust': { personalLinkAnchor: () => anchor },
    './personal-link-vault': { PersonalLinkVault: { open: async () => vault } },
  }, { Buffer, process, Date, setTimeout, clearTimeout, setImmediate, Uint8Array })
}
const principal = (alive = { value: true }, sessionId = 's1') => ({ namespace: 'c'.repeat(64), seal: { epoch: 1, sessionId, userId: 'u' }, current: () => alive.value }) as unknown as Principal

test('resposta perdida de RA e de MR retoma os MESMOS envelopes, sem segunda mutação', async () => {
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial(), p = principal()
  await link.startRoot(p, 'fixture', profile)
  const first = link.snapshot()!
  assert.equal(first.phase, 'confirmRoot')
  assert.equal(first.hubFingerprint!.replaceAll(' ', '').length, 32)
  hub.lose.add('root-accept')
  await link.confirm(p, first.attemptId)
  assert.equal(link.snapshot()!.phase, 'closed')
  assert.equal(hub.mutations, 1)
  await link.startRoot(p, 'fixture', profile)
  assert.equal(link.snapshot()!.phase, 'rootComplete')
  assert.ok(v.files.get('admission')!.equals(w.j(hub.ra!)), 'RA byte-exato recuperado')
  assert.equal(hub.mutations, 2, 'só RC além do RA original')
  await link.startManagement(p, 'fixture', profile)
  assert.equal(link.snapshot()!.phase, 'confirmManagement')
  hub.lose.add('management-accept')
  await link.confirm(p, link.snapshot()!.attemptId)
  assert.equal(link.snapshot()!.phase, 'closed')
  await link.startManagement(p, 'fixture', profile)
  assert.equal(link.snapshot()!.phase, 'managed')
  assert.equal(hub.mutations, 3)
  assert.ok(v.files.get('receipt')!.equals(w.j(hub.mr!)) && v.files.get('management')!.equals(w.j(hub.m!)))
})

test('troca de época durante await encerra sem assinatura, envio ou estado posteriores', async () => {
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial(), alive = { value: true }
  let released = false
  hub.hold = () => { alive.value = false; link.stop(); released = true }
  await link.startRoot(principal(alive), 'fixture', profile)
  await settle()
  assert.ok(released)
  assert.equal(link.snapshot(), null)
  assert.equal(v.counters.signs, 0)
  assert.equal(hub.frames, 1, 'nenhum envio depois do primeiro')
  assert.ok(hub.children.every(c => c.killed), 'sessão encerrada')
})

test('confirmação por outro principal ou depois de stop() não assina nem envia', async () => {
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial(), p = principal()
  await link.startRoot(p, 'fixture', profile)
  const { attemptId } = link.snapshot()!
  const frames = hub.frames
  await assert.rejects(link.confirm(principal({ value: true }, 'outra-sessao'), attemptId))
  link.stop()
  await assert.rejects(link.confirm(p, attemptId))
  assert.equal(v.counters.signs, 0)
  assert.equal(hub.frames, frames)
})

test('uncertain: cliques repetidos só reconciliam; zero novos desafios, assinatura ou mutação', async () => {
  const p = principal()
  // Raiz: aceite selado e enviado, mas a conexão cai ANTES de o Hub recebê-lo (resultado desconhecido).
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial()
  await link.startRoot(p, 'fixture', profile)
  hub.drop.add('root-accept')
  await link.confirm(p, link.snapshot()!.attemptId)
  assert.equal(link.snapshot()!.phase, 'closed')
  assert.equal(hub.mutations, 0)
  const signs = v.counters.signs, seals = v.counters.seals
  hub.ops.length = 0
  for (let i = 0; i < 3; i++) {
    await link.startRoot(p, 'fixture', profile)
    assert.equal(link.snapshot()!.reason, 'uncertain')
  }
  assert.deepEqual(hub.ops, ['reconcile', 'reconcile', 'reconcile'], 'nenhum root-challenge novo')
  assert.equal(v.counters.signs, signs)
  assert.equal(v.counters.seals, seals)
  assert.equal(hub.mutations, 0)
  // Gestão: complete + consentimento pendente fica incerta, sem management-challenge.
  const hub2 = fakeHub(), v2 = vaultMock(), m2 = load(hub2, v2.vault), link2 = new m2.PersonalLinkInitial()
  await link2.startRoot(p, 'fixture', profile)
  await link2.confirm(p, link2.snapshot()!.attemptId)
  assert.equal(link2.snapshot()!.phase, 'rootComplete')
  await link2.startManagement(p, 'fixture', profile)
  hub2.drop.add('management-accept')
  await link2.confirm(p, link2.snapshot()!.attemptId)
  const signs2 = v2.counters.signs, seals2 = v2.counters.seals, base = hub2.mutations
  hub2.ops.length = 0
  for (let i = 0; i < 2; i++) {
    await link2.startManagement(p, 'fixture', profile)
    assert.equal(link2.snapshot()!.reason, 'uncertain')
  }
  assert.deepEqual(hub2.ops, ['reconcile', 'reconcile'], 'nenhum management-challenge novo')
  assert.equal(v2.counters.signs, signs2)
  assert.equal(v2.counters.seals, seals2)
  assert.equal(hub2.mutations, base)
})

test('PRA-PRE-05: resposta canônica com id, op ou forma errados fecha a tentativa (sem pendurar)', { timeout: 10_000 }, async () => {
  for (const bad of ['id', 'op', 'shape'] as const) {
    const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial()
    hub.corrupt.set('reconcile', bad)
    let settled = false
    const run = link.startRoot(principal(), 'fixture', profile).then(() => { settled = true })
    await Promise.race([run, new Promise(resolve => setTimeout(resolve, 1_000))])
    assert.ok(settled, `${bad}: startRoot precisa terminar, não ficar pendente`)
    assert.equal(link.snapshot()?.phase, 'closed', bad)
    assert.equal(link.snapshot()?.reason, 'refused', bad)
    assert.equal(v.counters.signs, 0, bad)
    assert.equal(hub.mutations, 0, bad)
    assert.deepEqual(hub.ops, ['reconcile'], `${bad}: nenhum envio depois da resposta inválida`)
    assert.ok(hub.children.every(c => c.killed), `${bad}: sessão SSH encerrada`)
  }
})

/** Cofre D430 REAL (módulo personal-link-vault) com SO simulado; conta a assinatura Ed25519 efetiva. */
function loadRealVault(hub: ReturnType<typeof fakeHub>) {
  const dir = fs.mkdtempSync(path.join(os.homedir(), '.personal-link-vault-test-'))
  const state = { available: true, signatures: 0 }
  const safeStorage = { isEncryptionAvailable: () => state.available, getSelectedStorageBackend: () => 'gnome_libsecret', encryptString: (t: string) => Buffer.from(`enc:${t}`), decryptString: (b: Buffer) => b.toString().slice(4) }
  const counted = { ...crypto, signerFromSeed(seed: Uint8Array) { const real = crypto.signerFromSeed(seed); return { publicKey: real.publicKey, sign(m: Uint8Array) { state.signatures++; return real.sign(m) } } } }
  const promises = new Proxy(fs.promises, { get(target, key) { return key === 'lstat' ? (p: string) => p === '/usr/bin/ssh' ? Promise.resolve({ isFile: () => true, uid: 0, mode: 0o755 }) : target.lstat(p) : Reflect.get(target, key) } })
  const storeBytes = Buffer.from(`@cert-authority ${anchor.principal} ${anchor.ca}\n`)
  const mod = loadMainModule<typeof import('./personal-link-initial.ts')>('fork-cloud/personal-link-initial', {
    'node:child_process': { spawn: hub.spawn }, 'node:crypto': nodeCrypto, 'node:path': path, './crypto': counted, './personal-link-wire': w,
    'node:fs': { ...fs, promises }, electron: { app: { getPath: () => dir }, safeStorage },
    './personal-ssh': { protectedParents: async () => {}, certificate: () => ({ until: 4_000_000_000_000 }), privateFile: async (p: string) => ({ bytes: p.endsWith('ca') ? storeBytes : Buffer.from('log'), dev: 1, ino: 1 }) },
    './personal-link-trust': { personalLinkAnchor: () => anchor },
  }, { Buffer, process, Date, setTimeout, clearTimeout, setImmediate, Uint8Array })
  return { mod, state, cleanup: () => fs.rmSync(dir, { recursive: true, force: true }) }
}

test('SEG-A-R1-04: cofre perdido entre preparo e gesto não gera assinatura nem envio', async () => {
  // Simulado: safeStorage/fs do SO são mocks; prova a lógica do consumidor, não Keychain/libsecret reais.
  {
    const hub = fakeHub(), real = loadRealVault(hub), link = new real.mod.PersonalLinkInitial(), p = principal()
    try {
      await link.startRoot(p, 'fixture', profile)
      assert.equal(link.snapshot()?.phase, 'confirmRoot')
      real.state.available = false
      const antes = real.state.signatures
      await link.confirm(p, link.snapshot()!.attemptId)
      assert.equal(real.state.signatures, antes, 'gesto após perda do cofre: zero assinatura efetiva')
      assert.ok(!hub.ops.includes('root-accept'), 'nenhum envio do aceite')
      assert.equal(link.snapshot()?.phase, 'closed')
    } finally { real.cleanup() }
  }
})

test('SEG-A-R1-04: cofre perdido entre a resposta do Hub e o PC não gera assinatura nem envio', async () => {
  {
    const hub = fakeHub(), real = loadRealVault(hub), link = new real.mod.PersonalLinkInitial(), p = principal()
    try {
      await link.startRoot(p, 'fixture', profile)
      let antes = -1
      hub.holdOn.set('persist-challenge', () => { real.state.available = false; antes = real.state.signatures })
      await link.confirm(p, link.snapshot()!.attemptId)
      assert.ok(antes >= 0, 'chegou à resposta do Hub')
      assert.equal(real.state.signatures, antes, 'PC após perda do cofre: zero assinatura efetiva')
      assert.ok(!hub.ops.includes('persist-confirm'), 'nenhum envio do PC')
      assert.equal(hub.mutations, 1, 'só a RA anterior à perda')
    } finally { real.cleanup() }
  }
})

test('D456: RA pendente fechada pela reabertura do Hub não retoma; novo gesto usa geração e T novos, sem tocar a antiga', async () => {
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial(), p = principal()
  await link.startRoot(p, 'fixture', profile)
  const t1 = link.snapshot()!.device
  hub.drop.add('persist-challenge')
  await link.confirm(p, link.snapshot()!.attemptId)
  assert.equal(link.snapshot()!.phase, 'closed')
  assert.ok(v.files.has('admission') && !v.files.has('complete'))
  const antiga = new Map([...v.files].map(([k, b]) => [k, Buffer.from(b)]))
  const ra1 = hub.ra
  hub.reopened = true
  hub.ops.length = 0
  await link.startRoot(p, 'fixture', profile)
  assert.equal(link.snapshot()!.phase, 'confirmRoot')
  assert.notEqual(link.snapshot()!.device, t1, 'T novo para a nova RA')
  assert.deepEqual(hub.ops, ['reconcile', 'root-challenge'], 'nenhum PC/desafio para a RA antiga')
  assert.equal(v.gens.length, 2)
  assert.deepEqual([...v.gens[0]].map(([k, b]) => [k, b.toString('hex')]), [...antiga].map(([k, b]) => [k, b.toString('hex')]), 'geração antiga intacta')
  await link.confirm(p, link.snapshot()!.attemptId)
  assert.equal(link.snapshot()!.phase, 'rootComplete')
  assert.deepEqual(hub.closed, [ra1], 'Hub encerrou a RA antiga na Tx da nova')
  assert.ok(v.gens[1].has('admission') && v.gens[1].has('complete'))
})

/** RA commitada no Hub, resposta perdida: aceite selado e enviado, SEM `admission` local. */
async function respostaDeRaPerdida() {
  const hub = fakeHub(), v = vaultMock(), { PersonalLinkInitial } = load(hub, v.vault), link = new PersonalLinkInitial(), p = principal()
  await link.startRoot(p, 'fixture', profile)
  hub.lose.add('root-accept')
  await link.confirm(p, link.snapshot()!.attemptId)
  assert.equal(link.snapshot()!.phase, 'closed')
  assert.ok(hub.ra && !v.files.has('admission') && [...v.files.keys()].some(k => k.startsWith('accept.')), 'aceite selado, RA não')
  return { hub, v, link, p }
}

test('SEG-A-R2-01: closed inválido sem admission local não abre geração, chave, desafio nem assinatura', async t => {
  const casos: [string, (hub: ReturnType<typeof fakeHub>) => w.J][] = [
    ['sem RA', () => ['closed']],
    ['admissionId divergente', hub => ['closed', id(16), hub.ra!]],
    ['RA de outra operação', hub => ['closed', hub.admission, hub.raTrocada(12, 'f'.repeat(64))]],
    ['RA de outra chave T', hub => ['closed', hub.admission, hub.raTrocada(7, crypto.b64url(crypto.signerFromSeed(crypto.newDeviceSeed()).publicKey))]],
    ['RA de outro contexto C', hub => ['closed', hub.admission, hub.raTrocada(4, 'd'.repeat(64))]],
  ]
  for (const [nome, resposta] of casos) {
    await t.test(nome, async () => {
      const { hub, v, link, p } = await respostaDeRaPerdida()
      const antes = { gens: v.gens.length, signs: v.counters.signs, seals: v.counters.seals, desafios: hub.ops.filter(o => o === 'root-challenge').length, ra: hub.ra }
      hub.reconcileWith = resposta(hub)
      await link.startRoot(p, 'fixture', profile)
      assert.equal(v.gens.length, antes.gens, 'mesma geração (nenhum next)')
      assert.equal(v.counters.signs, antes.signs, 'nenhuma assinatura nova')
      assert.equal(v.counters.seals, antes.seals, 'nada selado como resultado')
      assert.equal(hub.ops.filter(o => o === 'root-challenge').length, antes.desafios, 'nenhum desafio novo')
      assert.equal(hub.ra, antes.ra)
      assert.equal(link.snapshot()?.phase, 'closed')
    })
  }
})

test('SEG-A-R2-01 controle: closed válido ligado ao aceite selado (D456) preserva histórico e prepara geração nova', async () => {
  const { hub, v, link, p } = await respostaDeRaPerdida()
  const antiga = new Map([...v.files].map(([k, b]) => [k, Buffer.from(b)]))
  hub.reconcileWith = ['closed', hub.admission, hub.ra!]
  hub.reopened = true
  await link.startRoot(p, 'fixture', profile)
  assert.equal(link.snapshot()?.phase, 'confirmRoot')
  assert.equal(v.gens.length, 2, 'geração nova')
  assert.deepEqual([...v.gens[0]].map(([k, b]) => [k, b.toString('hex')]), [...antiga].map(([k, b]) => [k, b.toString('hex')]), 'histórico intacto')
  assert.ok(!v.gens[0].has('admission'), 'RA antiga não é retomada nem selada como resultado')
})
