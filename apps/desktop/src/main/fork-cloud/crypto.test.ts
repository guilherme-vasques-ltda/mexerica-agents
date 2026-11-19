import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  b64url,
  channelKeys,
  computerIdOf,
  deviceHello,
  ephemeral,
  HandshakeError,
  hkdfExtract,
  hs1Input,
  cidRaw,
  openFrame,
  sasCode,
  sasCommit,
  sealFrame,
  signatureHeader,
  signerFromSeed,
  transcriptHash,
  unb64url,
  verifyEd25519,
} from './crypto.ts'

// Os mesmos vetores consumidos pelo Hub (Rust), pelo Worker (TS) e pelos apps móveis.
const vectors = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8'))
const u = (s: string) => unb64url(s)!

test('identidade, assinatura de request e aperto de mão batem com os vetores oficiais', () => {
  const k = vectors.keys
  const device = signerFromSeed(u(k.deviceSignSeed))
  const host = signerFromSeed(u(k.hostSignSeed))
  assert.equal(b64url(device.publicKey), k.deviceSignPub)
  assert.equal(b64url(host.publicKey), k.hostSignPub)
  assert.equal(computerIdOf(host.publicKey), k.computerId)
  assert.equal(b64url(ephemeral(u(k.hostBoxPriv)).pub), k.hostBoxPub)

  const r = vectors.requestSig
  const header = signatureHeader(device, r.method, new URL(`https://${r.authority}${r.pathAndQuery}`), new Uint8Array(), r.ts, r.nonce)
  assert.equal(`Codync-Sig: ${header}`, r.header)

  const h = vectors.handshake
  const hostPub = u(k.hostSignPub)
  const cid = cidRaw(hostPub)
  assert.equal(b64url(hs1Input(cid, device.publicKey, u(h.deviceEphPub), u(h.n))), h.hs1SignInput)
  const d = deviceHello(device, hostPub, { eph: ephemeral(u(h.deviceEphPriv)), n: u(h.n) })
  assert.equal(d.hello.sig, h.hs1Sig)
  assert.equal(d.hello.ek, h.deviceEphPub)
  const th = transcriptHash(cid, device.publicKey, u(h.deviceEphPub), u(h.n), u(h.hostEphPub))
  assert.equal(b64url(th), h.transcriptHash)
  assert.equal(b64url(host.sign(th)), h.hs2Sig)
  const ss = ephemeral(u(h.hostEphPriv)).dh(u(h.deviceEphPub))
  assert.equal(b64url(ss), h.sharedSecret)
  assert.equal(b64url(hkdfExtract(th, ss)), h.prk)
  const keys = d.finish({ ek: h.hostEphPub, sig: h.hs2Sig })
  assert.equal(b64url(keys.d2h), h.kD2H)
  assert.equal(b64url(keys.h2d), h.kH2D)
  assert.deepEqual(channelKeys(ss, th), keys)
})

test('welcome assinado por outra chave ou malformado é recusado (Hub errado / diretório adulterado)', () => {
  const k = vectors.keys
  const h = vectors.handshake
  const device = signerFromSeed(u(k.deviceSignSeed))
  const d = deviceHello(device, u(k.hostSignPub), { eph: ephemeral(u(h.deviceEphPriv)), n: u(h.n) })
  const impostor = signerFromSeed(new Uint8Array(32).fill(9))
  const th = transcriptHash(cidRaw(u(k.hostSignPub)), device.publicKey, u(h.deviceEphPub), u(h.n), u(h.hostEphPub))
  assert.throws(() => d.finish({ ek: h.hostEphPub, sig: b64url(impostor.sign(th)) }), /identidade deste Hub/)
  assert.throws(() => d.finish({ ek: 'curto', sig: h.hs2Sig }), /malformado/)
  assert.equal(verifyEd25519(u(k.hostSignPub), u(h.hs2Sig), th), true)
})

test('quadros: vetores, adulteração e contador trocado', () => {
  const h = vectors.handshake
  for (const f of vectors.frames) {
    const key = u(f.dir === 'd2h' ? h.kD2H : h.kH2D)
    assert.equal(sealFrame(key, f.c, f.final, new TextEncoder().encode(f.message)), f.d)
    const opened = openFrame(key, f.c, f.d)
    assert.equal(opened.final, f.final)
    assert.equal(new TextDecoder().decode(opened.chunk), f.message)
  }
  const f = vectors.frames[0]
  const key = u(h.kD2H)
  assert.throws(() => openFrame(key, f.c + 1, f.d), 'replay com outro contador não abre')
  const tampered = u(f.d)
  tampered[0] ^= 1
  assert.throws(() => openFrame(key, f.c, b64url(tampered)), 'tag não confere')
})

test('código de 6 dígitos e commit do pedido de acesso', () => {
  const s = vectors.sas
  const dk = u(vectors.keys.deviceSignPub)
  assert.equal(b64url(sasCommit(dk, u(s.deviceNonce))), s.commit)
  assert.equal(sasCode(u(vectors.keys.hostSignPub), dk, u(s.deviceNonce), u(s.hostNonce)), s.code)
})

test('código de confirmação (MX-149) recalculado pela fórmula do contrato bate com os vetores', () => {
  const c = vectors.accessConfirm
  const s = vectors.sas
  const dk = u(vectors.keys.deviceSignPub)
  const calc = (hostSignPub: Uint8Array, requestId: string) => {
    const id = Buffer.from(requestId, 'utf8')
    const h = createHash('sha256').update(Buffer.concat([
      Buffer.from('donna/access-confirm/v1'), hostSignPub, dk, u(s.deviceNonce), u(s.hostNonce), Buffer.from([id.length]), id,
    ])).digest()
    return String(h.readUInt32BE(0) % 1_000_000).padStart(6, '0')
  }
  const host = u(vectors.keys.hostSignPub)
  assert.equal(calc(host, c.requestId), c.code)
  assert.equal(calc(host, c.otherRequestId), c.otherCode)
  assert.equal(calc(dk, c.requestId), c.wrongKeyCode)
  assert.notEqual(c.wrongKeyCode, c.code, 'chave falsa não reproduz o código verdadeiro')
  assert.notEqual(c.code, vectors.sas.code, 'domínio diferente do SAS: os dois códigos são independentes')
})

test('base64url estrito recusa padding, lixo e tamanho errado', () => {
  assert.equal(unb64url('AA=='), null)
  assert.equal(unb64url('A$'), null)
  assert.equal(unb64url(vectors.keys.hostSignPub, 16), null)
  assert.equal(unb64url(42), null)
})

// Electron/BoringSSL não fornece a cifra nativa. Isolar a fronteira no filho.
test('quadros sem cifra nativa conservam os vetores e a autenticação', () => {
  const source = new URL('./crypto.ts', import.meta.url).href
  const fixture = new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url).href
  const script = `
    import assert from 'node:assert/strict';
    import crypto from 'node:crypto';
    import { syncBuiltinESMExports } from 'node:module';
    import { readFileSync } from 'node:fs';
    crypto.createCipheriv = crypto.createDecipheriv = () => { throw new Error('ERR_CRYPTO_UNKNOWN_CIPHER'); };
    syncBuiltinESMExports();
    const wire = await import(${JSON.stringify(source)});
    const v = JSON.parse(readFileSync(new URL(${JSON.stringify(fixture)}), 'utf8'));
    for (const f of v.frames) {
      const key = wire.unb64url(f.dir === 'd2h' ? v.handshake.kD2H : v.handshake.kH2D);
      assert.equal(wire.sealFrame(key, f.c, f.final, new TextEncoder().encode(f.message)), f.d);
      assert.equal(new TextDecoder().decode(wire.openFrame(key, f.c, f.d).chunk), f.message);
      assert.throws(() => wire.openFrame(key, f.c + 1, f.d));
      const bad = wire.unb64url(f.d); bad[0] ^= 1;
      assert.throws(() => wire.openFrame(key, f.c, wire.b64url(bad)));
    }
  `
  execFileSync(process.execPath, ['--experimental-strip-types', '--input-type=module', '-e', script], { env: {}, timeout: 10_000, stdio: 'pipe' })
})

test('HandshakeError é classe exportada (valor e tipo) e a mesma do protocolo compartilhado', async () => {
  const shared = await import('../../shared/relay-protocol/protocol.ts')
  const error: HandshakeError = new HandshakeError('x')
  assert.ok(error instanceof Error && error instanceof shared.HandshakeError)
  assert.equal(HandshakeError, shared.HandshakeError)
})
