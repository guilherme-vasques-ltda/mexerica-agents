import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { nodeBackend } from './backend-node.ts'
import { nobleBackend } from './backend-noble.ts'
import { createProtocol, type Protocol } from './protocol.ts'

// Os mesmos vetores consumidos pelo Hub (Rust), pelo Worker (TS) e pelos apps móveis —
// conferidos nos DOIS backends (node:crypto e @noble/*) e cruzados entre si.
const vectors = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8'))
const enc = new TextEncoder()
const dec = new TextDecoder()

const backends: [string, () => Protocol][] = [
  ['node:crypto', () => createProtocol(nodeBackend)],
  ['@noble/*', () => createProtocol(nobleBackend)],
]

function checkVectors(p: Protocol) {
  const k = vectors.keys
  const device = p.signerFromSeed(p.unb64url(k.deviceSignSeed)!)
  const host = p.signerFromSeed(p.unb64url(k.hostSignSeed)!)
  assert.equal(p.b64url(device.publicKey), k.deviceSignPub)
  assert.equal(p.b64url(host.publicKey), k.hostSignPub)
  assert.equal(p.computerIdOf(host.publicKey), k.computerId)
  assert.equal(p.b64url(p.ephemeral(p.unb64url(k.hostBoxPriv)!).pub), k.hostBoxPub)

  const r = vectors.requestSig
  const header = p.signatureHeader(device, r.method, new URL(`https://${r.authority}${r.pathAndQuery}`), new Uint8Array(), r.ts, r.nonce)
  assert.equal(`Codync-Sig: ${header}`, r.header)

  const h = vectors.handshake
  const hostPub = p.unb64url(k.hostSignPub)!
  const cid = p.cidRaw(hostPub)
  assert.equal(p.b64url(p.hs1Input(cid, device.publicKey, p.unb64url(h.deviceEphPub)!, p.unb64url(h.n)!)), h.hs1SignInput)
  const d = p.deviceHello(device, hostPub, { eph: p.ephemeral(p.unb64url(h.deviceEphPriv)!), n: p.unb64url(h.n)! })
  assert.equal(d.hello.sig, h.hs1Sig)
  assert.equal(d.hello.ek, h.deviceEphPub)
  const th = p.transcriptHash(cid, device.publicKey, p.unb64url(h.deviceEphPub)!, p.unb64url(h.n)!, p.unb64url(h.hostEphPub)!)
  assert.equal(p.b64url(th), h.transcriptHash)
  assert.equal(p.b64url(host.sign(th)), h.hs2Sig)
  const ss = p.ephemeral(p.unb64url(h.hostEphPriv)!).dh(p.unb64url(h.deviceEphPub)!)
  assert.equal(p.b64url(ss), h.sharedSecret)
  assert.equal(p.b64url(p.hkdfExtract(th, ss)), h.prk)
  const keys = d.finish({ ek: h.hostEphPub, sig: h.hs2Sig })
  assert.equal(p.b64url(keys.d2h), h.kD2H)
  assert.equal(p.b64url(keys.h2d), h.kH2D)
  assert.deepEqual(p.channelKeys(ss, th), keys)
}

for (const [name, make] of backends) {
  test(`vetores oficiais passam no backend ${name}`, () => {
    const p = make()
    checkVectors(p)

    // welcome assinado por outra chave ou malformado é recusado
    const k = vectors.keys
    const h = vectors.handshake
    const device = p.signerFromSeed(p.unb64url(k.deviceSignSeed)!)
    const d = p.deviceHello(device, p.unb64url(k.hostSignPub)!, { eph: p.ephemeral(p.unb64url(h.deviceEphPriv)!), n: p.unb64url(h.n)! })
    const impostor = p.signerFromSeed(new Uint8Array(32).fill(9))
    const th = p.transcriptHash(p.cidRaw(p.unb64url(k.hostSignPub)!), device.publicKey, p.unb64url(h.deviceEphPub)!, p.unb64url(h.n)!, p.unb64url(h.hostEphPub)!)
    assert.throws(() => d.finish({ ek: h.hostEphPub, sig: p.b64url(impostor.sign(th)) }), /identidade deste Hub/)
    assert.throws(() => d.finish({ ek: 'curto', sig: h.hs2Sig }), /malformado/)
    assert.equal(p.verifyEd25519(p.unb64url(k.hostSignPub)!, p.unb64url(h.hs2Sig)!, th), true)

    // quadros: vetores, adulteração e contador trocado
    for (const f of vectors.frames) {
      const key = p.unb64url(f.dir === 'd2h' ? h.kD2H : h.kH2D)!
      assert.equal(p.sealFrame(key, f.c, f.final, enc.encode(f.message)), f.d)
      const opened = p.openFrame(key, f.c, f.d)
      assert.equal(opened.final, f.final)
      assert.equal(dec.decode(opened.chunk), f.message)
    }
    const f = vectors.frames[0]
    const key = p.unb64url(h.kD2H)!
    assert.throws(() => p.openFrame(key, f.c + 1, f.d), 'replay com outro contador não abre')
    const tampered = p.unb64url(f.d)!
    tampered[0] ^= 1
    assert.throws(() => p.openFrame(key, f.c, p.b64url(tampered)), 'tag não confere')

    // código de 6 dígitos e commit do pedido de acesso
    const s = vectors.sas
    const dk = p.unb64url(vectors.keys.deviceSignPub)!
    assert.equal(p.b64url(p.sasCommit(dk, p.unb64url(s.deviceNonce)!)), s.commit)
    assert.equal(p.sasCode(p.unb64url(vectors.keys.hostSignPub)!, dk, p.unb64url(s.deviceNonce)!, p.unb64url(s.hostNonce)!), s.code)

    // base64url estrito recusa padding, lixo e tamanho errado
    assert.equal(p.unb64url('AA=='), null)
    assert.equal(p.unb64url('A$'), null)
    assert.equal(p.unb64url(vectors.keys.hostSignPub, 16), null)
    assert.equal(p.unb64url(42), null)
  })
}

// O protocolo é o mesmo nos dois sentidos: o que um backend assina/ sela o outro verifica/abre.
const node = createProtocol(nodeBackend)
const noble = createProtocol(nobleBackend)

test('assinatura, hello e quadros trocam entre node:crypto e @noble/*', () => {
  const k = vectors.keys
  const h = vectors.handshake
  const seed = node.unb64url(k.deviceSignSeed)!
  const hostPub = node.unb64url(k.hostSignPub)!

  // Mesma semente → mesma chave e mesma assinatura (Ed25519 determinístico).
  const dNode = node.signerFromSeed(seed)
  const dNoble = noble.signerFromSeed(seed)
  assert.equal(node.b64url(dNode.publicKey), noble.b64url(dNoble.publicKey))
  const msg = enc.encode('mensagem cruzada')
  assert.equal(node.b64url(dNode.sign(msg)), noble.b64url(dNoble.sign(msg)))

  // hello montado no backend web, conferido no backend node (e vice-versa).
  const ephPriv = node.unb64url(h.deviceEphPriv)!
  const n = node.unb64url(h.n)!
  const helloWeb = noble.deviceHello(dNoble, hostPub, { eph: noble.ephemeral(ephPriv), n })
  const helloNode = node.deviceHello(dNode, hostPub, { eph: node.ephemeral(ephPriv), n })
  assert.equal(helloWeb.hello.sig, helloNode.hello.sig)
  assert.equal(helloWeb.hello.sig, h.hs1Sig)

  const keysWeb = helloWeb.finish({ ek: h.hostEphPub, sig: h.hs2Sig })
  const keysNode = helloNode.finish({ ek: h.hostEphPub, sig: h.hs2Sig })
  assert.equal(node.b64url(keysWeb.d2h), node.b64url(keysNode.d2h))
  assert.equal(node.b64url(keysWeb.h2d), node.b64url(keysNode.h2d))

  // Quadro selado num backend abre no outro, nas duas direções.
  for (const f of vectors.frames) {
    const keyNode = node.unb64url(f.dir === 'd2h' ? h.kD2H : h.kH2D)!
    const keyNoble = noble.unb64url(f.dir === 'd2h' ? h.kD2H : h.kH2D)!
    const plaintext = enc.encode(f.message)
    const sealedByNode = node.sealFrame(keyNode, f.c, f.final, plaintext)
    const sealedByNoble = noble.sealFrame(keyNoble, f.c, f.final, plaintext)
    assert.equal(sealedByNode, sealedByNoble)
    assert.equal(dec.decode(noble.openFrame(keyNoble, f.c, sealedByNode).chunk), f.message)
    assert.equal(dec.decode(node.openFrame(keyNode, f.c, sealedByNoble).chunk), f.message)
  }

  // Assinatura de request gerada num backend verifica no outro.
  const device = noble.signerFromSeed(seed)
  const url = new URL(`https://${vectors.requestSig.authority}${vectors.requestSig.pathAndQuery}`)
  const r = vectors.requestSig
  const header = noble.signatureHeader(device, r.method, url, new Uint8Array(), r.ts, r.nonce)
  const fields = Object.fromEntries(header.split(',').map((p) => p.split('=') as [string, string]))
  const canonical = node.sigCanonical(r.method, url.host, url.pathname + url.search, r.ts, fields.nonce!, new Uint8Array())
  assert.ok(node.verifyEd25519(dNode.publicKey, node.unb64url(fields.sig, 64)!, enc.encode(canonical)))
})
