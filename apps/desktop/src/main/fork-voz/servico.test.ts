import { sessaoXai, type PedidoXai } from '../../shared/fork-voz-xai.ts'
import assert from 'node:assert/strict'
import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto'
import { promises as fsp, mkdtempSync, readdirSync, readFileSync, existsSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import { fonteEfetiva, textoDoResultado } from '../../renderer/views/fork/voz/chave-aparelho.ts'
import { tradutor } from '../../renderer/lib/fork-i18n/index.ts'
import type { Mensagem } from '../../renderer/views/fork/bot-settings/mensagem.ts'
import { textosVoz } from '../../renderer/views/fork/voz/frases.ts'

const pt = (partes: Mensagem[], mac = true) => textosVoz(tradutor('pt-BR'), partes, undefined, mac)
import type { VozBridge } from '../../shared/fork-voz.ts'
import { CofreVoz, type CofreDoSistema } from './cofre.ts'
import type { Buscar } from './provedor.ts'
import { instalarCanaisVoz, ServicoVoz } from './servico.ts'

// voz-04 (D70): a chave deste aparelho pela superfície real do preload (`preload/fork-voz.ts`), com
// `ipcRenderer` ligado aos canais reais do main. Só o cofre do sistema (AES em memória), o disco
// (pasta temporária) e o provedor (fetch fictício) são de mentira: nenhuma rede nem API paga.

const CHAVE = 'sk-proj-FICTICIA0123456789abcdefghijklmnopqrstuvwxyz'
const PEDACO = CHAVE.slice(9, 25)

/** Cofre do sistema fictício, mas que cifra de verdade (o arquivo nunca tem a chave em claro). */
function cofreDoSistema(backend = 'gnome_libsecret', disponivel = true) {
  const segredo = randomBytes(32)
  const conta = { cifrou: 0, decifrou: 0, travado: false }
  const sistema: CofreDoSistema = {
    isEncryptionAvailable: () => disponivel,
    getSelectedStorageBackend: () => backend,
    encryptString(texto) {
      conta.cifrou++
      const iv = randomBytes(12)
      const c = createCipheriv('aes-256-gcm', segredo, iv)
      const dados = Buffer.concat([c.update(texto, 'utf8'), c.final()])
      return Buffer.concat([iv, c.getAuthTag(), dados])
    },
    decryptString(b) {
      conta.decifrou++
      if (conta.travado) throw new Error(`Keychain locked while reading ${CHAVE}`)
      const d = createDecipheriv('aes-256-gcm', segredo, b.subarray(0, 12))
      d.setAuthTag(b.subarray(12, 28))
      return Buffer.concat([d.update(b.subarray(28)), d.final()]).toString('utf8')
    },
  }
  return { sistema, conta }
}

/** Provedor fictício: registra cada pedido e responde pelo `responder` do teste. */
function provedor(responder: (url: string, init: { method: string; body?: string }) => { status: number; body: string } | Promise<{ status: number; body: string }>) {
  const pedidos: { url: string; headers: Record<string, string>; body?: string }[] = []
  const buscar: Buscar = async (url, init) => {
    pedidos.push({ url, headers: init.headers, body: init.body })
    const r = await responder(url, init)
    return { status: r.status, text: async () => r.body }
  }
  return { buscar, pedidos }
}
const aceita = (url: string) => ({ status: 200, body: url.includes('client_secrets') ? '{"value":"ek_CURTA_DA_SESSAO"}' : url.includes('auth_tokens') ? '{"name":"auth_tokens/curta"}' : '{"data":[]}' })

/** Disco real numa pasta temporária, registrando cada `sync` de pasta. `pastaComoWindows`: abrir a pasta falha (EISDIR). */
type Etapa = 'abrir' | 'sync' | 'fechar'
type FalhasDisco = { rename?: boolean; rm?: boolean; pasta?: Etapa; vezes?: number; pular?: number; marcador?: Etapa | 'rename'; leituraMarcador?: boolean; rmMarcador?: boolean; reporMarcador?: boolean; restaurarMarcador?: boolean }
function disco(falharRm = false, pastaComoWindows = false, falhas: FalhasDisco = {}) {
  const sincronizadas: string[] = []
  const eventos: string[] = []
  const eio = () => Object.assign(new Error('EIO'), { code: 'EIO' })
  const fs = {
    mkdir: fsp.mkdir,
    readFile: (async (p: Parameters<typeof fsp.readFile>[0]) => {
      if (falhas.leituraMarcador && String(p).endsWith('.pendente')) throw eio()
      return fsp.readFile(p)
    }) as typeof fsp.readFile,
    rename: (async (a, b) => {
      eventos.push(`rename:${b}`)
      const destino = String(b)
      const marcador = destino.endsWith('.pendente')
      const reposicao = falhas.reporMarcador && eventos.includes(`rm:${b}`)
      const restauracao = falhas.restaurarMarcador && eventos.filter(e => e === `rename:${b}`).length > 1
      if ((falhas.rename && destino.endsWith('.bin')) ||
        (marcador && (falhas.marcador === 'rename' || reposicao || restauracao))) throw eio()
      return fsp.rename(a, b)
    }) as typeof fsp.rename,
    rm: (async (p, o) => { eventos.push(`rm:${p}`); if (falhas.rmMarcador && String(p).endsWith('.pendente')) throw eio(); if ((falharRm || falhas.rm) && String(p).endsWith('.bin')) throw Object.assign(new Error('EACCES'), { code: 'EACCES' }); return fsp.rm(p, o) }) as typeof fsp.rm,
    open: (async (p: string, flags?: string, mode?: number) => {
      eventos.push(`open:${p}`)
      if (pastaComoWindows && flags === 'r') throw Object.assign(new Error('EISDIR: illegal operation on a directory'), { code: 'EISDIR' })
      const falhar = (etapa: FalhasDisco['pasta']) => {
        if (flags !== 'r' || falhas.pasta !== etapa || falhas.vezes === 0) return
        if (falhas.pular) { falhas.pular--; return }
        if (falhas.vezes !== undefined) falhas.vezes--
        throw eio()
      }
      const marcador = p.includes('.pendente.')
      if (marcador && falhas.marcador === 'abrir') throw eio()
      falhar('abrir')
      const h = await fsp.open(p, flags, mode)
      if (flags !== 'r') return Object.assign(Object.create(h), {
        writeFile: (b: Buffer) => h.writeFile(b),
        sync: async () => { eventos.push(`sync:${p}`); if (marcador && falhas.marcador === 'sync') throw eio(); await h.sync() },
        close: async () => { await h.close(); if (marcador && falhas.marcador === 'fechar') throw eio() },
      })
      return Object.assign(Object.create(h), { sync: async () => { sincronizadas.push(p); eventos.push(`sync:${p}`); falhar('sync'); return h.sync() }, close: async () => { await h.close(); falhar('fechar') } })
    }) as typeof fsp.open,
  }
  return { fs, sincronizadas, eventos }
}

const preload = buildSync({
  entryPoints: [new URL('../../preload/fork-voz.ts', import.meta.url).pathname],
  bundle: true, platform: 'node', format: 'cjs', write: false, external: ['electron'],
}).outputFiles[0]!.text

interface Montagem {
  ponte: VozBridge
  pasta: string
  logs: string[]
  conta: ReturnType<typeof cofreDoSistema>['conta']
  pedidos: ReturnType<typeof provedor>['pedidos']
  sincronizadas: string[]
  eventos: string[]
  avisos: { n: number }
  sistema: CofreDoSistema
  recriar(): void
  invocar(canal: string, ...args: unknown[]): Promise<unknown>
}

/** Main (canais reais + serviço real) e preload real, ligados por um `ipcRenderer` fictício. */
function montar(opcoes: { plataforma?: NodeJS.Platform; backend?: string; disponivel?: boolean; permitido?: boolean; falhas?: FalhasDisco; falharRm?: boolean; pastaComoWindows?: boolean; responder?: Parameters<typeof provedor>[0] } = {}): Montagem {
  const pasta = mkdtempSync(join(tmpdir(), 'voz-04-'))
  const { sistema, conta } = cofreDoSistema(opcoes.backend, opcoes.disponivel ?? true)
  const { buscar, pedidos } = provedor(opcoes.responder ?? aceita)
  const { fs, sincronizadas, eventos } = disco(opcoes.falharRm, opcoes.pastaComoWindows, opcoes.falhas)
  const avisos = { n: 0 }
  const handlers = new Map<string, (event: unknown, ...args: unknown[]) => unknown>()
  const recriar = () => {
    const cofre = new CofreVoz({ dados: pasta, sistema, plataforma: opcoes.plataforma ?? 'darwin', fs })
    instalarCanaisVoz({ handle: (c, f) => handlers.set(c, f) }, new ServicoVoz(cofre, buscar, () => avisos.n++), () => opcoes.permitido ?? true)
  }
  recriar()
  const ouvintes = new Map<string, Set<() => void>>()
  const ipcRenderer = {
    invoke: async (canal: string, ...args: unknown[]) => {
      const h = handlers.get(canal)
      if (!h) throw new Error(`canal desconhecido: ${canal}`)
      // Como o Electron: o que volta passa por clonagem estruturada.
      return structuredClone(await h({}, ...args))
    },
    on: (c: string, f: () => void) => { if (!ouvintes.has(c)) ouvintes.set(c, new Set()); ouvintes.get(c)!.add(f) },
    removeListener: (c: string, f: () => void) => ouvintes.get(c)?.delete(f),
  }
  const logs: string[] = []
  const registrar = (...a: unknown[]) => logs.push(a.map((x) => (x instanceof Error ? `${x.message} ${x.stack}` : typeof x === 'string' ? x : JSON.stringify(x))).join(' '))
  const module = { exports: {} as { vozBridge: VozBridge } }
  runInNewContext(preload, { module, exports: module.exports, require: (n: string) => (n === 'electron' ? { ipcRenderer } : undefined), console: { log: registrar, info: registrar, warn: registrar, error: registrar, debug: registrar } })
  return { ponte: module.exports.vozBridge, pasta, logs, conta, pedidos, sincronizadas, eventos, avisos, sistema, recriar, invocar: async (c, ...args) => handlers.get(c)!({}, ...args) }
}

const arquivosDe = (pasta: string) => (existsSync(join(pasta, 'voz-chaves')) ? readdirSync(join(pasta, 'voz-chaves')) : [])

/** Procura o texto em qualquer campo do valor (respostas, estados, erros). */
function contem(raiz: unknown, texto: string, vistos = new Set<unknown>()): boolean {
  if (typeof raiz === 'string') return raiz.includes(texto)
  if (!raiz || typeof raiz !== 'object' || vistos.has(raiz)) return false
  vistos.add(raiz)
  if (raiz instanceof Error && (raiz.message.includes(texto) || String(raiz.stack).includes(texto))) return true
  return Object.values(raiz).some((v) => contem(v, texto, vistos))
}

test('a ponte do preload tem somente os canais fixos autorizados e o aviso, e nenhum devolve a chave', async () => {
  const m = montar()
  assert.deepEqual(Object.keys(m.ponte).sort(), ['catalogoXai', 'conferirSessaoXai', 'credencial', 'credencialXai', 'definirAtalho', 'estado', 'fecharSessaoXai', 'guardar', 'limpar', 'onChange', 'testar', 'usarReserva'])
  const respostas: unknown[] = []
  respostas.push(await m.ponte.guardar('openai', `  ${CHAVE}  `))
  respostas.push(await m.ponte.estado())
  respostas.push(await m.ponte.testar('openai'))
  respostas.push(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'))
  respostas.push(await m.ponte.usarReserva('openai'))
  respostas.push(await m.ponte.limpar('openai'))
  respostas.push(await m.ponte.estado())
  for (const r of respostas) assert.equal(contem(r, PEDACO), false, `nenhum trecho da chave em ${JSON.stringify(r)}`)
  assert.deepEqual(respostas[0], { ok: true, categoria: null, estado: { cofre: 'seguro', chaves: { openai: { situacao: 'guardada', reserva: false }, gemini: { situacao: 'ausente', reserva: false }, xai: { situacao: 'ausente', reserva: false } } } })
  assert.deepEqual(respostas[3], { ok: true, credential: 'ek_CURTA_DA_SESSAO' }, 'a credencial curta é a única coisa que volta')
  assert.equal(m.logs.some((l) => l.includes(PEDACO)), false, 'nada da chave no log')
  // A chave foi ao provedor só no cabeçalho, aparada, e o pedido não leva corpo do renderer.
  assert.equal(m.pedidos[0]!.headers.Authorization, `Bearer ${CHAVE}`)
})

test('o arquivo em disco é cifrado: a chave não aparece em claro', async () => {
  const m = montar()
  assert.equal((await m.ponte.guardar('gemini', CHAVE)).ok, true)
  assert.deepEqual(arquivosDe(m.pasta), ['gemini.bin'], 'um arquivo, sem temporário sobrando')
  const bruto = readFileSync(join(m.pasta, 'voz-chaves', 'gemini.bin'))
  assert.equal(bruto.includes(Buffer.from(CHAVE)), false)
  assert.equal(bruto.toString('latin1').includes(PEDACO), false)
  assert.equal(bruto.toString('base64').includes(Buffer.from(PEDACO).toString('base64').slice(0, 12)), false)
  assert.equal((await fsp.stat(join(m.pasta, 'voz-chaves', 'gemini.bin'))).mode & 0o777, 0o600)
  assert.ok(m.sincronizadas.some((p) => p.endsWith('voz-chaves')), 'a pasta foi sincronizada depois do rename')
})

test('guardar recusado pelo provedor não grava nada nem deixa temporário, e o erro não ecoa a chave', async () => {
  const casos = [
    [401, 'chave', `Incorrect API key provided: ${CHAVE}`],
    [400, 'chave', `API key not valid: ${CHAVE}`],
    [402, 'saldo', `insufficient_quota for ${CHAVE}`],
    [429, 'limite', `Rate limit reached for ${CHAVE}`],
    [500, 'provedor', `upstream failed while using ${CHAVE}`],
  ] as const
  for (const [status, categoria, mensagem] of casos) {
    const m = montar({ responder: () => ({ status, body: JSON.stringify({ error: { message: mensagem } }) }) })
    const r = await m.ponte.guardar('openai', CHAVE)
    assert.equal(r.ok, false)
    assert.equal(r.categoria, categoria, `HTTP ${status}`)
    assert.equal(contem(r, PEDACO), false)
    assert.deepEqual(arquivosDe(m.pasta), [], 'nenhum arquivo, nem temporário')
    assert.equal(m.conta.cifrou, 0, 'nem chegou a cifrar')
    assert.equal(r.estado.chaves.openai.situacao, 'ausente')
  }
  // Sem rede: também não grava.
  const buscarQueFalha = montar({ responder: () => { throw new Error(`ECONNREFUSED ${CHAVE}`) } })
  const r = await buscarQueFalha.ponte.guardar('openai', CHAVE)
  assert.equal(r.categoria, 'rede')
  assert.equal(contem(r, PEDACO), false)
  assert.deepEqual(arquivosDe(buscarQueFalha.pasta), [])
})

test('Linux com basic_text, unknown ou sem cofre recusa e nada é gravado; com o Chaveiro do GNOME, grava', async () => {
  for (const [backend, disponivel] of [['basic_text', true], ['unknown', true], ['gnome_libsecret', false]] as const) {
    const m = montar({ plataforma: 'linux', backend, disponivel })
    const r = await m.ponte.guardar('openai', CHAVE)
    assert.deepEqual([r.ok, r.categoria, r.estado.cofre], [false, 'sem-cofre', 'sem-cofre'], backend)
    assert.deepEqual(arquivosDe(m.pasta), [], `${backend}: nada gravado`)
    assert.equal(m.pedidos.length, 0, `${backend}: a chave nem vai ao provedor`)
    assert.equal(m.conta.cifrou, 0)
  }
  for (const backend of ['gnome_libsecret', 'kwallet5']) {
    const m = montar({ plataforma: 'linux', backend })
    assert.equal((await m.ponte.guardar('openai', CHAVE)).ok, true, backend)
  }
})

test('Limpar apaga o arquivo, sincroniza a pasta e descarta a cópia em memória', async () => {
  const m = montar()
  await m.ponte.guardar('openai', CHAVE)
  await m.ponte.guardar('gemini', CHAVE)
  m.sincronizadas.length = 0
  const r = await m.ponte.limpar('openai')
  assert.deepEqual([r.ok, r.estado.chaves.openai.situacao, r.estado.chaves.gemini.situacao], [true, 'ausente', 'guardada'])
  assert.deepEqual(arquivosDe(m.pasta), ['gemini.bin'], 'o outro provedor fica')
  assert.ok(m.sincronizadas.some((p) => p.endsWith('voz-chaves')), 'pasta sincronizada')
  // Sem a cópia em memória: a credencial não sai, e nada é decifrado para isso.
  const antes = m.conta.decifrou
  const c = await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
  assert.deepEqual(c, { ok: false, categoria: 'sem-chave' })
  assert.equal(m.conta.decifrou, antes)
  assert.equal(m.pedidos.some((p) => p.url.includes('client_secrets')), false, 'nenhum pedido ao provedor sem chave')
})

test('Limpar que não consegue apagar não diz que apagou', async () => {
  const m = montar({ falharRm: true })
  await m.ponte.guardar('openai', CHAVE)
  const r = await m.ponte.limpar('openai')
  assert.deepEqual([r.ok, r.categoria], [false, 'cofre'])
  assert.equal(r.estado.chaves.openai.situacao, 'limpeza-pendente', 'o arquivo ainda está lá, mas a chave não pode voltar ao uso')
})

test('o canal da credencial confere remetente, provedor, modelo e voz, e só devolve credencial ou categoria', async () => {
  const negado = montar({ permitido: false })
  assert.deepEqual(await negado.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'origem' })
  assert.deepEqual((await negado.ponte.guardar('openai', CHAVE)).categoria, 'origem')
  assert.deepEqual(arquivosDe(negado.pasta), [])
  assert.equal(negado.pedidos.length, 0)

  const m = montar({ responder: (url) => (url.includes('client_secrets') ? { status: 401, body: `{"error":{"message":"Incorrect API key provided: ${CHAVE}"}}` } : aceita(url)) })
  await m.ponte.guardar('openai', CHAVE)
  await m.ponte.guardar('gemini', CHAVE)
  const invalida = { ok: false, categoria: 'invalida' }
  const invoke = m.ponte.credencial as (...a: unknown[]) => Promise<unknown>
  for (const args of [['xai', 'm', 'v'], ['openai', '../x', 'marin'], ['openai', 'gpt-realtime', 'a?b'], ['openai', 'gpt-realtime', 'a#b'], ['openai', 'gpt-realtime'], ['openai', 'm'.repeat(101), 'marin'], ['gemini', 'gemini-1.5-pro', 'Kore']]) {
    assert.deepEqual(await invoke(...args), invalida, JSON.stringify(args))
  }
  assert.equal(m.pedidos.filter((p) => !p.url.includes('/models')).length, 0, 'nada formatado errado vai ao provedor')
  const recusada = await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
  assert.deepEqual(recusada, { ok: false, categoria: 'chave' }, 'só a categoria, nunca o corpo do provedor')
  assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'recusada', 'presente e recusada, não ausente')
  assert.deepEqual(arquivosDe(m.pasta).sort(), ['gemini.bin', 'openai.bin'], 'o arquivo é preservado')
  const gemini = await m.ponte.credencial('gemini', 'gemini-3.8-live', 'Kore')
  assert.deepEqual(gemini, { ok: true, credential: 'auth_tokens/curta' })
  assert.equal(contem(m.logs, PEDACO), false)
})

test('chave travada: estado próprio, arquivo preservado, nenhuma credencial; a reserva só por escolha', async () => {
  const m = montar()
  await m.ponte.guardar('openai', CHAVE)
  m.conta.travado = true
  const c = await m.ponte.testar('openai') // "Tentar de novo": abre do zero
  assert.deepEqual([c.ok, c.categoria, c.estado.chaves.openai.situacao], [false, 'cofre', 'travada'])
  assert.equal(contem(c, PEDACO), false, 'o texto do sistema (que trazia a chave) não sai')
  assert.deepEqual(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'cofre' })
  assert.deepEqual(arquivosDe(m.pasta), ['openai.bin'])
  assert.equal((await m.ponte.estado()).chaves.openai.reserva, false, 'sem escolha, nada de Hub')
  const escolha = await m.ponte.usarReserva('openai')
  assert.deepEqual([escolha.ok, escolha.estado.chaves.openai.reserva], [true, true])
  assert.equal((await m.ponte.usarReserva('gemini')).ok, false, 'sem chave travada, não há o que escolher')
  // Limpar desfaz a escolha junto com a chave.
  const limpo = await m.ponte.limpar('openai')
  assert.deepEqual(limpo.estado.chaves.openai, { situacao: 'ausente', reserva: false })
  assert.equal(contem(m.logs, PEDACO), false)
})

test('Windows: sem fsync de pasta (abrir a pasta falha lá), guardar e Limpar dão certo; no Mac a mesma falha não vira sucesso', async () => {
  const w = montar({ plataforma: 'win32', pastaComoWindows: true })
  const g = await w.ponte.guardar('openai', CHAVE)
  assert.deepEqual([g.ok, g.estado.chaves.openai.situacao], [true, 'guardada'])
  const l = await w.ponte.limpar('openai')
  assert.deepEqual([l.ok, l.estado.chaves.openai.situacao], [true, 'ausente'])
  assert.deepEqual(arquivosDe(w.pasta), [])
  const m = montar({ plataforma: 'darwin', pastaComoWindows: true })
  assert.deepEqual((await m.ponte.guardar('openai', CHAVE)).categoria, 'cofre', 'no Mac, sem sincronizar a pasta não se anuncia guardada')
})

test('a escolha do Hub continua depois que o cofre volta a abrir; só a pessoa volta à chave daqui', async () => {
  const m = montar()
  await m.ponte.guardar('gemini', CHAVE)
  m.conta.travado = true
  await m.ponte.testar('gemini')
  assert.equal((await m.ponte.usarReserva('gemini')).ok, true)
  // O chaveiro do sistema destrava sozinho: a chave abre, mas a escolha do Hub continua.
  m.conta.travado = false
  const e = await m.ponte.estado()
  assert.deepEqual(e.chaves.gemini, { situacao: 'guardada', reserva: true }, 'não passa a cobrar a chave daqui em silêncio')
  // Uma credencial pedida por engano não desfaz a escolha.
  await m.ponte.credencial('gemini', 'gemini-3.8-live', 'Kore')
  assert.equal((await m.ponte.estado()).chaves.gemini.reserva, true)
  // "Usar a chave deste Mac" (Testar com sucesso): a pessoa volta à chave daqui.
  const volta = await m.ponte.testar('gemini')
  assert.deepEqual([volta.ok, volta.estado.chaves.gemini.reserva], [true, false])
})

const registroDe = (m: Montagem) => JSON.parse(readFileSync(join(m.pasta, 'voz-chaves', 'openai.pendente'), 'utf8'))
const cifradoDe = (m: Montagem) => readFileSync(join(m.pasta, 'voz-chaves', 'openai.bin'))
function conferirRegistro(m: Montagem, op: 'guardar' | 'limpar') {
  const registro = registroDe(m)
  assert.deepEqual(Object.keys(registro).sort(), ['id', 'op'])
  assert.equal(registro.op, op)
  assert.match(registro.id, /^[a-f0-9]{32}$/)
  assert.equal(contem(registro, PEDACO), false)
  assert.equal(statSync(join(m.pasta, 'voz-chaves', 'openai.pendente')).mode & 0o777, 0o600)
  return registro
}
async function bloqueada(m: Montagem, situacao: 'incerta' | 'limpeza-pendente') {
  const e = await m.ponte.estado()
  assert.deepEqual(e.chaves.openai, { situacao, reserva: false })
  assert.equal(fonteEfetiva('tempo-real', e.chaves.openai, 'hub'), situacao, 'Hub nunca entra sem escolha')
  const antes = m.pedidos.length
  assert.deepEqual(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'cofre' })
  assert.equal(m.pedidos.length, antes, 'nenhum pedido ao provedor com chave bloqueada')
}

// Reproduções da revisão: o segundo erro fica armado e denuncia qualquer tentativa de reposição.
for (const op of ['guardar', 'limpar'] as const) {
  test(`rodada 4: ${op} confirmado com falha dupla ao retirar marcador responde resultado real e reinicia igual`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    m.eventos.length = 0
    falhas.pasta = 'sync'; falhas.pular = 2; falhas.vezes = 1
    falhas.reporMarcador = true // EIO no rename se o código tentar repor após o primeiro EIO.
    const r = await (op === 'guardar' ? m.ponte.guardar('openai', `${CHAVE}nova`) : m.ponte.limpar('openai'))
    assert.equal(r.ok, true)
    assert.deepEqual(r.estado.chaves.openai, { situacao: op === 'guardar' ? 'guardada' : 'ausente', reserva: false })
    assert.equal(arquivosDe(m.pasta).includes('openai.bin'), op === 'guardar')
    m.recriar()
    assert.deepEqual((await m.ponte.estado()).chaves.openai, r.estado.chaves.openai)
    assert.equal(m.eventos.filter(e => e.endsWith('openai.pendente') && e.startsWith('rename:')).length, 1, 'nenhuma reposição do marcador')
    delete falhas.pasta
  })

  test(`${op}: rm do marcador falha depois da confirmação, marcador velho não muda o resultado`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    falhas.rmMarcador = true
    const r = await (op === 'guardar' ? m.ponte.guardar('openai', `${CHAVE}nova`) : m.ponte.limpar('openai'))
    assert.equal(r.ok, true)
    const registro = conferirRegistro(m, op)
    if (op === 'guardar') {
      const bruto = cifradoDe(m)
      const aberto = JSON.parse(m.sistema.decryptString(bruto))
      assert.equal(aberto.id, registro.id, 'id fica junto da chave dentro do cifrado autenticado')
      assert.equal(aberto.chave, `${CHAVE}nova`)
      assert.equal(bruto.includes(Buffer.from(registro.id)), false, 'id não fica em claro ao lado da chave')
    }
    m.recriar()
    assert.deepEqual((await m.ponte.estado()).chaves.openai, r.estado.chaves.openai)
    if (op === 'guardar') assert.equal((await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')).ok, true)
    else assert.deepEqual(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'sem-chave' })
    falhas.rmMarcador = false
    m.recriar()
    assert.deepEqual((await m.ponte.estado()).chaves.openai, r.estado.chaves.openai)
    assert.equal(arquivosDe(m.pasta).includes('openai.pendente'), false)
  })
}

for (const etapa of ['abrir', 'sync', 'fechar'] as const) {
  test(`Guardar: falha de ${etapa} após rename bloqueia; reinício reconhece id da chave nova`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    falhas.pasta = etapa; falhas.pular = 1; falhas.rm = true
    const r = await m.ponte.guardar('openai', `${CHAVE}nova`)
    assert.equal(r.ok, false)
    conferirRegistro(m, 'guardar')
    await bloqueada(m, 'incerta')
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'guardada')
    assert.equal(JSON.parse(m.sistema.decryptString(cifradoDe(m))).chave, `${CHAVE}nova`)
    assert.equal((await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')).ok, true)
    assert.equal(m.pedidos.at(-1)!.headers.Authorization, `Bearer ${CHAVE}nova`)
  })

  for (const persistente of [true, false]) {
    test(`Limpar: ${etapa} após remoção, erro ${persistente ? 'persistente' : 'transitório'}, recriação termina só com confirmação`, async () => {
      const falhas: FalhasDisco = {}
      const m = montar({ falhas })
      await m.ponte.guardar('openai', CHAVE)
      await m.ponte.guardar('gemini', CHAVE)
      falhas.pasta = etapa; falhas.pular = 1
      const r = await m.ponte.limpar('openai')
      assert.equal(r.ok, false)
      conferirRegistro(m, 'limpar')
      await bloqueada(m, 'limpeza-pendente')
      assert.equal((await m.ponte.testar('openai')).ok, false)
      if (!persistente) delete falhas.pasta
      m.recriar()
      if (persistente) {
        await bloqueada(m, 'limpeza-pendente')
        assert.equal((await m.ponte.usarReserva('openai')).ok, false)
        delete falhas.pasta
        m.recriar()
      }
      assert.deepEqual((await m.ponte.estado()).chaves.openai, { situacao: 'ausente', reserva: false })
      assert.equal((await m.ponte.estado()).chaves.gemini.situacao, 'guardada')
      assert.deepEqual(arquivosDe(m.pasta), ['gemini.bin'])
    })
  }
}

for (const anterior of [false, true]) {
  test(`Guardar interrompido antes da chave, ${anterior ? 'com' : 'sem'} chave anterior: id diferente preserva disco no reinício`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    if (anterior) await m.ponte.guardar('openai', CHAVE)
    const bytes = anterior ? cifradoDe(m) : null
    falhas.rename = true; falhas.rmMarcador = true
    const r = await m.ponte.guardar('openai', `${CHAVE}nova`)
    assert.equal(r.ok, false)
    const registro = conferirRegistro(m, 'guardar')
    if (bytes) assert.notEqual(JSON.parse(m.sistema.decryptString(bytes)).id, registro.id)
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, anterior ? 'guardada' : 'ausente')
    if (bytes) assert.deepEqual(cifradoDe(m), bytes)
    else assert.equal(arquivosDe(m.pasta).includes('openai.bin'), false)
    falhas.rmMarcador = false
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, anterior ? 'guardada' : 'ausente')
  })
}

for (const op of ['guardar', 'limpar'] as const) {
  test(`${op}: falha antes de escrever marcador preserva chave e diz Nada mudou`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    const anterior = cifradoDe(m)
    falhas.marcador = 'abrir'
    const r = await (op === 'guardar' ? m.ponte.guardar('openai', `${CHAVE}nova`) : m.ponte.limpar('openai'))
    assert.deepEqual([r.ok, r.categoria, r.estado.chaves.openai.situacao], [false, 'registro', 'guardada'])
    assert.match(pt(textoDoResultado(op, r, null, r.estado.chaves.openai.situacao).partes, true), /Nada mudou/)
    assert.deepEqual(cifradoDe(m), anterior)
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'guardada')
  })
}

for (const etapa of ['abrir', 'sync', 'fechar'] as const) {
  test(`rodada 4: marcador Limpar sem confirmação (${etapa}) e restauração impossível mantém intenção, nunca Nada mudou`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    // Deixa um registro Guardar anterior, como na reprodução independente.
    falhas.rmMarcador = true
    await m.ponte.guardar('openai', CHAVE)
    const anterior = cifradoDe(m)
    falhas.rmMarcador = false
    m.eventos.length = 0
    falhas.pasta = etapa; falhas.restaurarMarcador = true
    const r = await m.ponte.limpar('openai')
    assert.deepEqual([r.ok, r.categoria], [false, 'cofre'])
    assert.equal(pt(textoDoResultado('limpar', r, null, r.estado.chaves.openai.situacao).partes, true), 'A chave está sendo apagada. Tente Limpar de novo.')
    assert.deepEqual(cifradoDe(m), anterior, 'a remoção ainda não começou')
    conferirRegistro(m, 'limpar')
    await bloqueada(m, 'limpeza-pendente')
    assert.equal(m.eventos.filter(e => e.endsWith('openai.pendente') && e.startsWith('rename:')).length, 1, 'nenhuma restauração do marcador anterior')
    m.recriar()
    await bloqueada(m, 'limpeza-pendente')
    delete falhas.pasta
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'ausente')
    assert.equal(arquivosDe(m.pasta).includes('openai.bin'), false)
  })
}

test('Guardar pendente com arquivo que não decifra fica incerta após recriar, sem credencial nem Hub', async () => {
  const falhas: FalhasDisco = { rmMarcador: true }
  const m = montar({ falhas })
  await m.ponte.guardar('openai', CHAVE)
  conferirRegistro(m, 'guardar')
  await fsp.writeFile(join(m.pasta, 'voz-chaves', 'openai.bin'), 'cifrado corrompido')
  m.recriar()
  await bloqueada(m, 'incerta')
  falhas.rmMarcador = false
  assert.equal((await m.ponte.limpar('openai')).ok, true)
  m.recriar()
  assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'ausente')
})

for (const posterior of ['limpar', 'trocar', 'outro-provedor'] as const) {
  test(`rodada 4: Guardar atrasado × ${posterior}, geração é conferida dentro da fila`, async () => {
    let iniciou!: () => void
    let liberar!: () => void
    const inicio = new Promise<void>(r => { iniciou = r })
    const livre = new Promise<void>(r => { liberar = r })
    let primeiro = true
    const m = montar({ responder: async (url) => {
      if (primeiro) { primeiro = false; iniciou(); await livre }
      return aceita(url)
    } })
    const guardar = m.ponte.guardar('openai', CHAVE)
    await inicio
    const r = await (posterior === 'trocar' ? m.ponte.guardar('openai', `${CHAVE}nova`) : m.ponte.limpar(posterior === 'limpar' ? 'openai' : 'gemini'))
    assert.equal(r.ok, true)
    liberar()
    const antigo = await guardar
    assert.deepEqual([antigo.ok, antigo.categoria], posterior === 'outro-provedor' ? [true, null] : [false, 'cancelado'])
    assert.equal(antigo.estado.chaves.openai.situacao, posterior === 'limpar' ? 'ausente' : 'guardada')
    m.recriar()
    assert.deepEqual((await m.ponte.estado()).chaves.openai, antigo.estado.chaves.openai)
    if (posterior !== 'limpar') assert.equal(JSON.parse(m.sistema.decryptString(cifradoDe(m))).chave, posterior === 'trocar' ? `${CHAVE}nova` : CHAVE)
    else assert.deepEqual(arquivosDe(m.pasta), [])
    assert.equal(contem(antigo, PEDACO), false)
  })
}

for (const conteudo of ['{', '{}', 'null', '{"op":"limpar","em":"inválida"}', '{"op":"outra","id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}', '{"op":"guardar","id":"inválido"}', 'ilegível']) {
  test(`marcador ${conteudo}: incerta, sem credencial nem Hub até Limpar confirmado`, async () => {
    const falhas: FalhasDisco = {}
    const m = montar({ falhas })
    await m.ponte.guardar('openai', CHAVE)
    await fsp.writeFile(join(m.pasta, 'voz-chaves', 'openai.pendente'), conteudo)
    falhas.leituraMarcador = conteudo === 'ilegível'
    m.recriar()
    const e = await m.ponte.estado()
    assert.deepEqual(e.chaves.openai, { situacao: 'incerta', reserva: false })
    assert.equal(fonteEfetiva('tempo-real', e.chaves.openai, 'hub'), 'incerta')
    assert.deepEqual(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'cofre' })
    assert.equal(m.pedidos.some(p => p.url.includes('client_secrets')), false)
    falhas.leituraMarcador = false
    assert.equal((await m.ponte.limpar('openai')).ok, true)
    m.recriar()
    assert.equal(fonteEfetiva('tempo-real', (await m.ponte.estado()).chaves.openai, 'hub'), 'hub')
  })
}

test('estado e credencial esperam a reconciliação terminar, inclusive a de outro provedor', async () => {
  const pasta = mkdtempSync(join(tmpdir(), 'voz-04-inicio-'))
  await fsp.mkdir(join(pasta, 'voz-chaves'))
  await fsp.writeFile(join(pasta, 'voz-chaves', 'gemini.pendente'), JSON.stringify({ op: 'limpar', id: 'a'.repeat(32) }))
  const { sistema } = cofreDoSistema()
  const { buscar, pedidos } = provedor(aceita)
  let iniciou!: () => void
  let liberar!: () => void
  const inicio = new Promise<void>(resolve => { iniciou = resolve })
  const liberado = new Promise<void>(resolve => { liberar = resolve })
  const cofre = new CofreVoz({ dados: pasta, sistema, plataforma: 'darwin', fs: {
    ...fsp,
    readFile: (async (p: Parameters<typeof fsp.readFile>[0]) => {
      if (String(p).endsWith('gemini.pendente')) { iniciou(); await liberado }
      return fsp.readFile(p)
    }) as typeof fsp.readFile,
  } })
  const servico = new ServicoVoz(cofre, buscar)
  let respondeu = false
  const estado = servico.estado().then(e => { respondeu = true; return e })
  const credencial = servico.credencial('openai', 'gpt-realtime-2.1', 'marin').then(c => { respondeu = true; return c })
  await inicio
  assert.equal(respondeu, false)
  assert.equal(pedidos.length, 0)
  liberar()
  assert.equal((await estado).chaves.gemini.situacao, 'ausente')
  assert.deepEqual(await credencial, { ok: false, categoria: 'sem-chave' })
  assert.deepEqual(arquivosDe(pasta), [])
})

test('guardar validado também é cancelado se Limpar chegar enquanto aguarda a fila inicial', async () => {
  const pasta = mkdtempSync(join(tmpdir(), 'voz-04-fila-'))
  let liberar!: () => void
  let enfileirou!: () => void
  const livre = new Promise<void>(r => { liberar = r })
  const enfileirado = new Promise<void>(r => { enfileirou = r })
  const { sistema } = cofreDoSistema()
  const cofre = new CofreVoz({ dados: pasta, sistema, plataforma: 'darwin', fs: {
    ...fsp,
    readFile: (async (p: Parameters<typeof fsp.readFile>[0]) => {
      if (String(p).endsWith('gemini.pendente')) await livre
      return fsp.readFile(p)
    }) as typeof fsp.readFile,
  } })
  const conferir = cofre.naGeracao.bind(cofre)
  cofre.naGeracao = (...args) => {
    const trabalho = conferir(...args)
    enfileirou()
    return trabalho
  }
  const servico = new ServicoVoz(cofre, provedor(aceita).buscar)
  // A validação termina, mas a reconciliação ainda retém a fila do cofre.
  const guardar = servico.guardar('openai', CHAVE)
  await enfileirado
  const limpar = servico.limpar('openai')
  liberar()
  assert.equal((await guardar).categoria, 'cancelado')
  assert.equal((await limpar).ok, true)
  assert.deepEqual(arquivosDe(pasta), [])
  const reiniciado = new ServicoVoz(new CofreVoz({ dados: pasta, sistema, plataforma: 'darwin' }), provedor(aceita).buscar)
  assert.deepEqual((await reiniciado.estado()).chaves.openai, { situacao: 'ausente', reserva: false })
})

test('chave do formato anterior continua cifrada e utilizável mesmo com marcador de outra operação', async () => {
  const m = montar()
  await m.ponte.guardar('openai', CHAVE)
  const bytes = m.sistema.encryptString(CHAVE)
  await fsp.writeFile(join(m.pasta, 'voz-chaves', 'openai.bin'), bytes)
  await fsp.writeFile(join(m.pasta, 'voz-chaves', 'openai.pendente'), JSON.stringify({ op: 'guardar', id: 'a'.repeat(32) }))
  m.recriar()
  assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'guardada')
  assert.deepEqual(cifradoDe(m), bytes, 'nenhuma migração ou regravação da chave antiga')
  assert.equal((await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')).ok, true)
  assert.equal(m.pedidos.at(-1)!.headers.Authorization, `Bearer ${CHAVE}`)
  assert.equal(arquivosDe(m.pasta).includes('openai.pendente'), false)
})

/** Retém uma resposta específica, deixando Trocar/Limpar e pedidos posteriores seguirem. */
function montarComRespostaRetida(status: number) {
  let iniciou!: () => void
  let liberar!: () => void
  const inicio = new Promise<void>(r => { iniciou = r })
  const livre = new Promise<void>(r => { liberar = r })
  const controle = { reter: false, statusLivre: 200 }
  const resposta = (url: string, codigo: number) => codigo === 200 ? aceita(url) : { status: codigo, body: `{"error":{"message":"Incorrect API key: ${CHAVE}"}}` }
  const m = montar({ responder: async url => {
    if (!controle.reter) return resposta(url, controle.statusLivre)
    controle.reter = false
    iniciou()
    await livre
    return resposta(url, status)
  } })
  return { m, controle, inicio, liberar }
}

for (const operacao of ['testar', 'credencial'] as const) {
  test(`rodada 5: 401 antiga de ${operacao} após Trocar não recusa a chave nova`, async () => {
    const { m, controle, inicio, liberar } = montarComRespostaRetida(401)
    await m.ponte.guardar('openai', CHAVE)
    controle.reter = true
    const antiga = operacao === 'testar' ? m.ponte.testar('openai') : m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
    await inicio
    assert.equal((await m.ponte.guardar('openai', `${CHAVE}nova`)).ok, true)
    const nova = cifradoDe(m)
    liberar()
    const r = await antiga
    assert.equal(r.ok, false)
    assert.deepEqual([r.ok, r.categoria], [false, 'cancelado'])
    assert.deepEqual((await m.ponte.estado()).chaves.openai, { situacao: 'guardada', reserva: false })
    assert.deepEqual(cifradoDe(m), nova)
    assert.equal((await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')).ok, true)
    assert.equal(m.pedidos.at(-1)!.headers.Authorization, `Bearer ${CHAVE}nova`)
    assert.equal(contem(r, PEDACO), false)
    m.recriar()
    assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'guardada')
  })
}

test('rodada 5: Testar com resposta de sucesso depois de Limpar responde cancelado', async () => {
  const { m, controle, inicio, liberar } = montarComRespostaRetida(200)
  await m.ponte.guardar('openai', CHAVE)
  controle.reter = true
  const antiga = m.ponte.testar('openai')
  await inicio
  assert.equal((await m.ponte.limpar('openai')).ok, true)
  liberar()
  const r = await antiga
  assert.deepEqual([r.ok, r.categoria, r.estado.chaves.openai], [false, 'cancelado', { situacao: 'ausente', reserva: false }])
  assert.deepEqual(arquivosDe(m.pasta), [])
  m.recriar()
  assert.equal((await m.ponte.estado()).chaves.openai.situacao, 'ausente')
})

for (const posterior of ['limpar', 'trocar'] as const) {
  test(`rodada 5: credencial de sucesso superada por ${posterior} nunca sai do main`, async () => {
    const { m, controle, inicio, liberar } = montarComRespostaRetida(200)
    await m.ponte.guardar('openai', CHAVE)
    controle.reter = true
    const antiga = m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
    await inicio
    const atual = await (posterior === 'limpar' ? m.ponte.limpar('openai') : m.ponte.guardar('openai', `${CHAVE}nova`))
    assert.equal(atual.ok, true)
    liberar()
    const r = await antiga
    assert.deepEqual(r, { ok: false, categoria: 'cancelado' })
    assert.equal(contem(r, 'ek_CURTA_DA_SESSAO'), false)
    assert.deepEqual((await m.ponte.estado()).chaves.openai, atual.estado.chaves.openai)
    m.recriar()
    assert.deepEqual((await m.ponte.estado()).chaves.openai, atual.estado.chaves.openai)
  })
}

for (const operacao of ['testar', 'credencial'] as const) {
  test(`rodada 5: sucesso antigo de ${operacao} não aceita a chave nova recusada nem desfaz escolha do Hub`, async () => {
    const { m, controle, inicio, liberar } = montarComRespostaRetida(200)
    await m.ponte.guardar('openai', CHAVE)
    controle.reter = true
    const antiga = operacao === 'testar' ? m.ponte.testar('openai') : m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
    await inicio
    assert.equal((await m.ponte.guardar('openai', `${CHAVE}nova`)).ok, true)
    controle.statusLivre = 401
    assert.deepEqual(await m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin'), { ok: false, categoria: 'chave' })
    const atual = await m.ponte.usarReserva('openai')
    assert.deepEqual(atual.estado.chaves.openai, { situacao: 'recusada', reserva: true })
    liberar()
    const r = await antiga
    assert.equal(r.ok, false)
    assert.deepEqual([r.ok, r.categoria], [false, 'cancelado'])
    assert.deepEqual((await m.ponte.estado()).chaves.openai, atual.estado.chaves.openai)
  })
}

test('rodada 5: Guardar recusado e superado por Limpar responde cancelado, sem recusa antiga', async () => {
  const { m, controle, inicio, liberar } = montarComRespostaRetida(401)
  controle.reter = true
  const antiga = m.ponte.guardar('openai', CHAVE)
  await inicio
  assert.equal((await m.ponte.limpar('openai')).ok, true)
  liberar()
  const r = await antiga
  assert.deepEqual([r.ok, r.categoria, r.estado.chaves.openai], [false, 'cancelado', { situacao: 'ausente', reserva: false }])
  assert.equal(m.conta.cifrou, 0)
  assert.deepEqual(arquivosDe(m.pasta), [])
  assert.equal(contem(r, PEDACO), false)
})

function sinal() {
  let resolver!: () => void
  const promessa = new Promise<void>(r => { resolver = r })
  return { promessa, resolver: () => resolver() }
}

for (const operacao of ['testar', 'credencial'] as const) {
  test(`rodada 5: ${operacao} aberto durante a validação de Trocar não afeta a chave publicada depois`, async () => {
    const iniciouTroca = sinal(), liberarTroca = sinal()
    const iniciouAntiga = sinal(), liberarAntiga = sinal()
    let reterTroca = false, reterAntiga = false
    const m = montar({ responder: async url => {
      if (reterTroca) {
        reterTroca = false
        iniciouTroca.resolver()
        await liberarTroca.promessa
      } else if (reterAntiga) {
        reterAntiga = false
        iniciouAntiga.resolver()
        await liberarAntiga.promessa
        return { status: 401, body: `Incorrect API key: ${CHAVE}` }
      }
      return aceita(url)
    } })
    await m.ponte.guardar('openai', CHAVE)
    reterTroca = true
    const troca = m.ponte.guardar('openai', `${CHAVE}nova`)
    await iniciouTroca.promessa
    reterAntiga = true
    const antiga = operacao === 'testar' ? m.ponte.testar('openai') : m.ponte.credencial('openai', 'gpt-realtime-2.1', 'marin')
    await iniciouAntiga.promessa
    assert.equal(m.pedidos.at(-1)!.headers.Authorization, `Bearer ${CHAVE}`)
    liberarTroca.resolver()
    assert.equal((await troca).ok, true)
    liberarAntiga.resolver()
    const r = await antiga
    assert.equal(r.ok, false)
    assert.equal(r.categoria, 'cancelado')
    assert.deepEqual((await m.ponte.estado()).chaves.openai, { situacao: 'guardada', reserva: false })
  })
}

for (const operacao of ['testar', 'credencial'] as const) {
  test(`rodada 5: ${operacao} com resposta já recebida espera a fila e cancela após Limpar`, async () => {
    const pasta = mkdtempSync(join(tmpdir(), 'voz-04-resposta-fila-'))
    const { sistema } = cofreDoSistema()
    const entrouProvedor = sinal(), liberarProvedor = sinal()
    const entrouFila = sinal(), liberarFila = sinal(), enfileirou = sinal()
    let reterProvedor = false, reterFila = false
    const cofre = new CofreVoz({ dados: pasta, sistema, plataforma: 'darwin', fs: {
      ...fsp,
      readFile: (async (p: Parameters<typeof fsp.readFile>[0]) => {
        if (reterFila && String(p).endsWith('gemini.bin')) {
          reterFila = false
          entrouFila.resolver()
          await liberarFila.promessa
        }
        return fsp.readFile(p)
      }) as typeof fsp.readFile,
    } })
    const { buscar } = provedor(async url => {
      if (reterProvedor) {
        reterProvedor = false
        entrouProvedor.resolver()
        await liberarProvedor.promessa
        if (operacao === 'testar') return { status: 401, body: 'Incorrect API key' }
      }
      return aceita(url)
    })
    const servico = new ServicoVoz(cofre, buscar)
    await servico.guardar('openai', CHAVE)
    reterProvedor = true
    const antiga = operacao === 'testar' ? servico.testar('openai') : servico.credencial('openai', 'gpt-realtime-2.1', 'marin')
    await entrouProvedor.promessa
    reterFila = true
    const consulta = servico.estado()
    await entrouFila.promessa
    const conferir = cofre.naGeracao.bind(cofre)
    cofre.naGeracao = (...args) => {
      const trabalho = conferir(...args)
      enfileirou.resolver()
      return trabalho
    }
    liberarProvedor.resolver()
    await enfileirou.promessa
    const limpar = servico.limpar('openai')
    liberarFila.resolver()
    await consulta
    assert.equal((await limpar).ok, true)
    const r = await antiga
    assert.equal(r.ok, false)
    assert.equal(r.categoria, 'cancelado')
    assert.deepEqual((await servico.estado()).chaves.openai, { situacao: 'ausente', reserva: false })
    assert.deepEqual(arquivosDe(pasta), [])
  })
}

test('rodada 5: publicar uma gravação em andamento preserva a vez de Trocar posterior', async () => {
  const pasta = mkdtempSync(join(tmpdir(), 'voz-04-troca-posterior-'))
  const { sistema } = cofreDoSistema()
  const renomeou = sinal(), liberar = sinal(), validouPosterior = sinal()
  let primeira = true
  const cofre = new CofreVoz({ dados: pasta, sistema, plataforma: 'darwin', fs: {
    ...fsp,
    rename: async (a, b) => {
      await fsp.rename(a, b)
      if (primeira && String(b).endsWith('openai.bin')) {
        primeira = false
        renomeou.resolver()
        await liberar.promessa
      }
    },
  } })
  const servico = new ServicoVoz(cofre, async (_url, init) => ({ status: 200, text: async () => {
    if (init.headers.Authorization === `Bearer ${CHAVE}nova`) validouPosterior.resolver()
    return '{"data":[]}'
  } }))
  const anterior = servico.guardar('openai', CHAVE)
  await renomeou.promessa
  const posterior = servico.guardar('openai', `${CHAVE}nova`)
  await validouPosterior.promessa
  liberar.resolver()
  assert.equal((await anterior).ok, true)
  assert.equal((await posterior).ok, true)
  assert.equal(await cofre.abrir('openai'), `${CHAVE}nova`)
})

// D382: a ElevenLabs saiu das integrações. Chave e marcador antigos ficam no disco, intocados e fora do estado.
test('elevenlabs.bin legado é ignorado sem erro, sem aparecer no estado e sem ser apagado', async () => {
  const m = montar()
  const dir = join(m.pasta, 'voz-chaves')
  await fsp.mkdir(dir, { recursive: true, mode: 0o700 })
  const bin = join(dir, 'elevenlabs.bin'), marcador = join(dir, 'elevenlabs.pendente')
  const bytes = m.sistema.encryptString(CHAVE)
  const registro = JSON.stringify({ op: 'limpar', id: 'a'.repeat(32) })
  await fsp.writeFile(bin, bytes)
  await fsp.writeFile(marcador, registro)
  m.recriar()
  const estado = await m.ponte.estado()
  assert.deepEqual(Object.keys(estado.chaves).sort(), ['gemini', 'openai', 'xai'])
  assert.equal(estado.cofre, 'seguro')
  assert.equal((await m.ponte.guardar('openai', CHAVE)).ok, true)
  assert.equal((await m.ponte.limpar('openai')).ok, true)
  for (const [canal, ...args] of [['voz:guardar', 'elevenlabs', CHAVE], ['voz:limpar', 'elevenlabs'], ['voz:testar', 'elevenlabs'], ['voz:credencial', 'elevenlabs', 'm', 'v']]) {
    assert.equal(((await m.invocar(canal!, ...args)) as { categoria: unknown }).categoria, 'invalida', canal)
  }
  assert.equal(await m.invocar('voz:elevenlabs:credencial', {}).catch(() => 'sem-canal'), 'sem-canal')
  assert.deepEqual(readFileSync(bin), bytes)
  assert.equal(readFileSync(marcador, 'utf8'), registro)
  assert.equal(m.eventos.some(e => e.includes('elevenlabs')), false)
})

const PEDIDO_XAI: PedidoXai = { model: 'grok-voice-3.0', voice: 'eve', language: 'pt-br', instructions: 'Somente operador DONNA, em português do Brasil.' }
const TOKEN_XAI = 'xai-realtime-client-secret-curto-zyxwvutsrq'
const aceitaXai = (url: string) => ({ status: 200, body: JSON.stringify(url.endsWith('/client_secrets')
  ? { value: TOKEN_XAI, expires_at: Math.floor(Date.now() / 1000) + 300 }
  : { voices: [{ voice_id: 'eve', name: 'Eve' }, { voice_id: 'ara', name: 'Ara' }] }) })
const negociadaXai = () => ({ id: 'sessao-xai', ...structuredClone(sessaoXai(PEDIDO_XAI)) })

test('voz-10: preload/main/cofre reais, catálogo e mint xAI sem sessão ou anchor; chave longa só no main', async () => {
  const m = montar({ responder: aceitaXai })
  assert.equal((await m.ponte.guardar('xai', CHAVE)).ok, true)
  assert.equal(readFileSync(join(m.pasta, 'voz-chaves', 'xai.bin')).includes(CHAVE), false)
  assert.deepEqual(await m.ponte.catalogoXai(), { ok: true, vozes: [{ id: 'eve', nome: 'Eve' }, { id: 'ara', nome: 'Ara' }] })
  const r = await m.ponte.credencialXai(PEDIDO_XAI); assert.equal(r.ok, true)
  if (!r.ok) return
  assert.equal(r.credential, TOKEN_XAI); assert.match(r.negociacao, /^[a-f0-9-]{36}$/)
  assert.deepEqual(JSON.parse(m.pedidos.at(-1)!.body!), { expires_after: { seconds: 300 } })
  assert.ok(m.pedidos.every(p => p.url.startsWith('https://api.x.ai/v1/')))
  assert.ok(m.pedidos.every(p => p.headers.Authorization === `Bearer ${CHAVE}`))
  const s = negociadaXai(); s.tools.reverse()
  assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, s), { ok: true })
  assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, { ...s, id: 'sessao-errada' }), { ok: false, categoria: 'provedor' })
  assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, s), { ok: false, categoria: 'cancelado' })
  assert.equal(contem(await m.ponte.estado(), CHAVE), false); assert.equal(contem(await m.ponte.estado(), TOKEN_XAI), false)
  assert.equal(contem(m.logs, CHAVE), false); assert.equal(contem(m.logs, TOKEN_XAI), false)
  assert.deepEqual(await m.ponte.credencial('xai', PEDIDO_XAI.model, PEDIDO_XAI.voice), { ok: false, categoria: 'invalida' })
  assert.equal((await m.ponte.usarReserva('xai')).ok, false)
  assert.deepEqual(await m.ponte.fecharSessaoXai(r.negociacao), { ok: true })
})

test('voz-10: sem chave não consulta provedor; sem cofre não grava nem cunha; nunca usa reserva', async () => {
  const m = montar({ responder: () => assert.fail('não pode haver rede sem chave') })
  assert.deepEqual(await m.ponte.catalogoXai(), { ok: false, categoria: 'sem-chave' })
  assert.deepEqual(await m.ponte.credencialXai(PEDIDO_XAI), { ok: false, categoria: 'sem-chave' })
  assert.equal((await m.ponte.usarReserva('xai')).ok, false)
  const semCofre = montar({ disponivel: false, responder: () => assert.fail('sem rede') })
  assert.equal((await semCofre.ponte.guardar('xai', CHAVE)).ok, false)
  assert.equal((await semCofre.ponte.credencialXai(PEDIDO_XAI)).ok, false)
})

test('voz-10: canais xAI rejeitam remetente, aridade, modelo, idioma e sessão arbitrários', async () => {
  const m = montar({ responder: () => assert.fail('recusa antes de qualquer rede') })
  for (const p of [null, [], { ...PEDIDO_XAI, model: 'grok-voice-latest' }, { ...PEDIDO_XAI, model: 'grok-voice-2.0' },
    { ...PEDIDO_XAI, language: '' }, { ...PEDIDO_XAI, voice: 'https://outra' }, { ...PEDIDO_XAI, tools: [] }]) {
    assert.deepEqual(await m.invocar('voz:xai:credencial', p), { ok: false, categoria: 'invalida' })
  }
  assert.deepEqual(await m.invocar('voz:xai:credencial', PEDIDO_XAI, 'extra'), { ok: false, categoria: 'invalida' })
  assert.deepEqual(await m.invocar('voz:xai:catalogo', 'extra'), { ok: false, categoria: 'invalida' })
  assert.deepEqual(await m.ponte.conferirSessaoXai('id-invalido', {}), { ok: false, categoria: 'invalida' })
  const negado = montar({ permitido: false, responder: () => assert.fail('remetente recusado') })
  assert.deepEqual(await negado.ponte.catalogoXai(), { ok: false, categoria: 'origem' })
  assert.deepEqual(await negado.ponte.credencialXai(PEDIDO_XAI), { ok: false, categoria: 'origem' })
  assert.deepEqual(await negado.ponte.conferirSessaoXai('11111111-1111-4111-8111-111111111111', {}), { ok: false, categoria: 'origem' })
  assert.deepEqual(await negado.ponte.fecharSessaoXai('11111111-1111-4111-8111-111111111111'), { ok: false, categoria: 'origem' })
})

test('voz-10: main recusa sessão divergente, incompleta e qualquer ferramenta do servidor', async () => {
  const m = montar({ responder: aceitaXai }); await m.ponte.guardar('xai', CHAVE)
  const mudancas = [
    (s: any) => { s.model = 'grok-voice-latest' }, (s: any) => { s.voice = 'ara' },
    (s: any) => { s.instructions += 'outra regra' }, (s: any) => { delete s.audio },
    (s: any) => { s.audio.input.transcription.language_hint = 'en' },
    (s: any) => { s.turn_detection.idle_timeout_ms = 1 }, (s: any) => { s.resumption.enabled = true },
    ...['web_search', 'x_search', 'file_search', 'mcp'].map(type => (s: any) => { s.tools.push({ type }) }),
    (s: any) => { s.tools[0].parameters = {} },
  ]
  for (const mudar of mudancas) {
    const r = await m.ponte.credencialXai(PEDIDO_XAI); assert.equal(r.ok, true); if (!r.ok) return
    const s = negociadaXai(); mudar(s)
    assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, s), { ok: false, categoria: 'provedor' })
    assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, negociadaXai()), { ok: false, categoria: 'cancelado' })
  }
})

test('voz-10: catálogo com eco de segredo, IDs duplicados ou corpo inválido não sai do main', async () => {
  for (const corpo of [
    { voices: [{ voice_id: 'eve', name: CHAVE }] }, { voices: [{ voice_id: 'eve', name: PEDACO }] },
    { voices: [{ voice_id: 'eve', name: 'Eve' }, { voice_id: 'eve', name: 'Duplicada' }] },
    { voices: [{ voice_id: 'https://errado', name: 'Outra' }] }, { voices: {} }, {}, null,
  ]) {
    let ruim = false
    const m = montar({ responder: url => ruim ? { status: 200, body: JSON.stringify(corpo) } : aceitaXai(url) })
    await m.ponte.guardar('xai', CHAVE); ruim = true
    const r = await m.ponte.catalogoXai(); assert.deepEqual(r, { ok: false, categoria: 'provedor' })
    assert.equal(contem(r, PEDACO), false); assert.equal(m.logs.length, 0)
  }
})

test('voz-10: expiração, segredo malformado, erro ecoado e voz ausente falham por categoria', async () => {
  for (const resposta of [
    { status: 200, body: JSON.stringify({ value: TOKEN_XAI, expires_at: 1 }) },
    { status: 200, body: JSON.stringify({ value: TOKEN_XAI, expires_at: Math.floor(Date.now() / 1000) + 3600 }) },
    { status: 200, body: JSON.stringify({ value: CHAVE, expires_at: Math.floor(Date.now() / 1000) + 300 }) },
    { status: 200, body: '{"value":"token com espaço"}' }, { status: 200, body: '{' },
    ...[401, 402, 429, 500].map(status => ({ status, body: `${CHAVE} ${TOKEN_XAI}` })),
  ]) {
    const m = montar({ responder: url => url.endsWith('/client_secrets') ? resposta : aceitaXai(url) })
    await m.ponte.guardar('xai', CHAVE)
    const r = await m.ponte.credencialXai(PEDIDO_XAI); assert.equal(r.ok, false)
    assert.equal(contem(r, CHAVE), false); assert.equal(contem(r, TOKEN_XAI), false); assert.equal(m.logs.length, 0)
  }
  const m = montar({ responder: aceitaXai }); await m.ponte.guardar('xai', CHAVE)
  assert.deepEqual(await m.ponte.credencialXai({ ...PEDIDO_XAI, voice: 'retirada' }), { ok: false, categoria: 'invalida' })
  assert.equal(m.pedidos.some(p => p.url.endsWith('/client_secrets')), false)
})

for (const etapa of ['catalogo', 'mint']) test(`voz-10: Limpar durante ${etapa} cancela credencial atrasada e impede a negociação`, async () => {
  let liberar!: () => void, entrou!: () => void
  const iniciou = new Promise<void>(r => { entrou = r })
  let armada = false
  const m = montar({ responder: async url => {
    if (armada && (etapa === 'catalogo' ? url.endsWith('/tts/voices') : url.endsWith('/client_secrets'))) {
      entrou(); await new Promise<void>(r => { liberar = r })
    }
    return aceitaXai(url)
  } })
  await m.ponte.guardar('xai', CHAVE); armada = true
  const r = m.ponte.credencialXai(PEDIDO_XAI); await iniciou; await m.ponte.limpar('xai'); liberar()
  assert.deepEqual(await r, { ok: false, categoria: 'cancelado' })
  if (etapa === 'catalogo') assert.equal(m.pedidos.some(p => p.url.endsWith('/client_secrets')), false)
})

test('voz-10: Limpar/Trocar e fechar invalidam a negociação; pedido original permanece fixo', async () => {
  for (const acao of ['limpar', 'trocar', 'fechar']) {
    const m = montar({ responder: aceitaXai }); await m.ponte.guardar('xai', CHAVE)
    const p = { ...PEDIDO_XAI }; const r = await m.ponte.credencialXai(p); if (!r.ok) assert.fail('mint fake')
    p.voice = 'ara'
    assert.deepEqual(await m.ponte.conferirSessaoXai(r.negociacao, negociadaXai()), { ok: true })
    if (acao === 'limpar') await m.ponte.limpar('xai')
    else if (acao === 'trocar') await m.ponte.guardar('xai', `${CHAVE}-outra`)
    else await m.ponte.fecharSessaoXai(r.negociacao)
    assert.equal((await m.ponte.conferirSessaoXai(r.negociacao, negociadaXai())).ok, false)
  }
})

test('voz-10: negociação vence sem confirmação, expira após 30 min e não mantém eco de segredo em ID', async t => {
  t.mock.timers.enable({ apis: ['Date'], now: new Date('2026-11-04T12:00:00Z') })
  const m = montar({ responder: aceitaXai }); await m.ponte.guardar('xai', CHAVE)
  const r = await m.ponte.credencialXai(PEDIDO_XAI); if (!r.ok) assert.fail('mint fake')
  t.mock.timers.tick(15_000)
  assert.equal((await m.ponte.conferirSessaoXai(r.negociacao, negociadaXai())).ok, false)
  for (const id of [CHAVE, TOKEN_XAI]) {
    const n = await m.ponte.credencialXai(PEDIDO_XAI); if (!n.ok) assert.fail('mint fake')
    assert.equal((await m.ponte.conferirSessaoXai(n.negociacao, { ...negociadaXai(), id })).ok, false)
  }
  const n = await m.ponte.credencialXai(PEDIDO_XAI); if (!n.ok) assert.fail('mint fake')
  assert.equal((await m.ponte.conferirSessaoXai(n.negociacao, negociadaXai())).ok, true)
  t.mock.timers.tick(1_800_000)
  assert.equal((await m.ponte.conferirSessaoXai(n.negociacao, negociadaXai())).ok, false)
})
