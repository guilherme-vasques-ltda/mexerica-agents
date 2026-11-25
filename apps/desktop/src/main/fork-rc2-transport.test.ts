import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { chmodSync, closeSync, mkdtempSync, openSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { before, test } from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import type { Rc2VerifiedArtifact } from './fork-rc2-artifact.ts'
import { Rc2HubLifecycle, type Rc2Deadlines, type Rc2Schedule } from './fork-rc2-local-hub.ts'
import { rc2BootstrapPort, rc2FdExecBase, rc2HelloPayload, rc2NativeTransport } from './fork-rc2-transport.ts'

// Prazos curtos: cada recusa da abertura é resolvida pelo prazo de hello, e a escalada de
// encerramento tem etapas de centenas de milissegundos, coerentes com a sonda de 20 ms.
const deadlines: Rc2Deadlines = { open: 3000, hello: 400, close: 1000, eof: 300, sigterm: 400, sigkill: 400, probe: 20 }

const fakeSchedule: Rc2Schedule = (ms, fire) => {
  const timer = setTimeout(fire, ms)
  return () => clearTimeout(timer)
}

type Componente = { role: 'hub'; path: string; fd: number }

function artefato(componente: Componente): Rc2VerifiedArtifact {
  return { components: [componente], close: () => { try { closeSync(componente.fd) } catch { /* já fechado */ } } }
}

const fonte = new URL('./test-support/rc2-filho-teste.c', import.meta.url)
const cenarios = ['', '-DCENARIO_LINHA_INVALIDA', '-DCENARIO_LINHA_EXTRA', '-DCENARIO_LINHA_ATRASADA', '-DCENARIO_PORTA_BAIXA', '-DCENARIO_BEARER_REJEITA',
  '-DCENARIO_NAO_ESCUTA', '-DCENARIO_MUDO', '-DCENARIO_IGNORA_SIGTERM', '-DCENARIO_DESCENDENTE'] as const
const chave = (cenario: string) => (cenario === '' ? 'filho-padrao' : `filho-${cenario.toLowerCase()}`)
let compiladorAusente = false
let binarios: Record<string, string> = {}

before(() => {
  const pasta = mkdtempSync(join(tmpdir(), 'rc2-filhos-'))
  for (const flags of cenarios) {
    const cenario = flags ? flags.slice('-DCENARIO_'.length) : ''
    const saida = join(pasta, `${chave(cenario)}.bin`)
    const resultado = spawnSync('cc', ['-O2', '-o', saida, fonte.pathname, ...(flags ? [flags] : [])])
    if (resultado.error) { compiladorAusente = true; return }
    if (resultado.status !== 0) throw new Error(`cc falhou para ${cenario || 'padrao'}: ${resultado.stderr}`)
    binarios[chave(cenario)] = saida
  }
})

function filho(t: { skip: (motivo: string) => unknown }, cenario: string): { caminho: string; fd: number } {
  const caminho = binarios[chave(cenario)]
  if (compiladorAusente) return void t.skip('sem compilador C neste ambiente: o vetor de processo real não roda') as never
  return { caminho, fd: openSync(caminho, 'r') }
}

function cicloDo(componente: Componente) {
  const sessao = { valor: {} as object }
  const transporte = rc2NativeTransport(artefato(componente))
  assert.ok(transporte !== null, 'linux tem base de execução por descritor')
  const ciclo = new Rc2HubLifecycle(() => sessao.valor, transporte, { deadlines, schedule: fakeSchedule })
  return { sessao, ciclo }
}

// Sonda externa de processos do binário: prova de que nada do filho sobrevive ao encerramento.
async function morreu(caminho: string, prazoMs: number): Promise<boolean> {
  const fim = Date.now() + prazoMs
  for (;;) {
    const vivos = readdirSync('/proc').filter(pid => /^\d+$/.test(pid)).filter(pid => {
      try { return realpathSync(`/proc/${pid}/exe`) === caminho } catch { return false }
    })
    if (vivos.length === 0) return true
    if (Date.now() > fim) return false
    await new Promise(retoma => setTimeout(retoma, 20))
  }
}

function source(url: string, imports: Record<string, unknown>, globals: Record<string, unknown> = {}) {
  const modulo = ts.transpileModule(readFileSync(new URL(url, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 },
  }).outputText
  const exports: Record<string, unknown> = {}
  runInNewContext(modulo, {
    exports, ...globals, require: (alvo: string) => Object.hasOwn(imports, alvo) ? imports[alvo] : assert.fail(`unexpected import: ${alvo}`),
  })
  return exports
}

test('base de execução por descritor só existe onde a execução direta do binário é comprovada', () => {
  assert.equal(rc2FdExecBase('linux'), '/proc/self/fd')
  for (const plataforma of ['darwin', 'win32', 'freebsd', 'sunos', '']) assert.equal(rc2FdExecBase(plataforma), null)
  // sem base ou sem componente de hub não há transporte: o perfil segue inerte
  assert.equal(rc2NativeTransport({ components: [], close: () => { } }), null)
  assert.equal(rc2NativeTransport({ components: [{ role: 'helper', path: 'ajudante', fd: 3 }], close: () => { } }), null)
  const noDarwin = source('./fork-rc2-transport.ts', {
    'node:child_process': { spawn: () => { } }, 'node:crypto': { randomBytes: () => Buffer.alloc(0) }, 'node:http': { request: () => { } },
  }, { process: { platform: 'darwin' } }).rc2NativeTransport as (artefato: Rc2VerifiedArtifact) => unknown
  assert.equal(noDarwin({ components: [{ role: 'hub', path: 'donnabot-host', fd: 3 }], close: () => { } }), null)
})

test('linha de bootstrap aceita só JSON estrito com porta não privilegiada', () => {
  assert.equal(rc2BootstrapPort('{"v":1,"port":45678}'), 45678)
  assert.equal(rc2BootstrapPort('{"v":1,"port":1024}'), 1024)
  for (const linha of [
    '', 'isto não é JSON', 'null', '[]', '"texto"', '{}', '{"v":1}', '{"port":45678}', '{"v":2,"port":45678}',
    '{"v":1,"port":80}', '{"v":1,"port":1023}', '{"v":1,"port":0}', '{"v":1,"port":65536}', '{"v":1,"port":70000}',
    '{"v":1,"port":45.5}', '{"v":1,"port":"45678"}', '{"v":1,"port":45678,"id":"extra"}',
    '{"v":"1","port":45678}',
  ]) assert.throws(() => rc2BootstrapPort(linha), /invalid_rc2_bootstrap/, JSON.stringify(linha))
})

test('resposta de hello aceita só identidades curtas com as duas chaves', () => {
  assert.deepEqual(rc2HelloPayload('{"computerId":"c","hostId":"h"}'), { computerId: 'c', hostId: 'h' })
  for (const corpo of [
    '', 'null', '[]', '{}', '{"computerId":"c"}', '{"hostId":"h"}', '{"computerId":"","hostId":"h"}',
    '{"computerId":"c","hostId":7}', '{"computerId":"c","hostId":"h","extra":1}', `{"computerId":"${'a'.repeat(257)}","hostId":"h"}`,
  ]) assert.throws(() => rc2HelloPayload(corpo), /invalid_rc2_bootstrap/, JSON.stringify(corpo))
})

test('fluxo completo com processo real: bootstrap, bearer da geração, hello, evento e encerramento confirmado', async t => {
  const { caminho, fd } = filho(t, '')
  const aberturas: AbortSignal[] = []
  const sessao = { valor: {} as object }
  const transporte = rc2NativeTransport(artefato({ role: 'hub', path: 'filho-padrao', fd }))!
  const ciclo = new Rc2HubLifecycle(() => sessao.valor,
    signal => { aberturas.push(signal); return transporte(signal) }, { deadlines, schedule: fakeSchedule })
  assert.equal(await ciclo.start(), true)
  assert.equal(aberturas.length, 1, 'uma geração, um spawn')
  assert.deepEqual(await ciclo.hello(), { computerId: 'filho-teste', hostId: 'filho-hub' })
  const recebidos: string[] = []
  const corrente = (async () => { for await (const evento of ciclo.events()) recebidos.push(evento) })()
  for (let espera = 0; recebidos.length === 0 && espera < 150; espera++) await new Promise(retoma => setTimeout(retoma, 20))
  assert.deepEqual(recebidos, ['{"type":"batida"}'])
  assert.equal(await ciclo.stop(), true, 'grupo morto confirmado pela sonda')
  await corrente
  assert.equal(aberturas[0].aborted, true)
  assert.ok(await morreu(caminho, 2000))
})

test('mudança de sessão encerra a geração viva e abre a nova; start durante drenagem é recusado', async t => {
  const { fd } = filho(t, '')
  const { sessao, ciclo } = cicloDo({ role: 'hub', path: 'filho-padrao', fd })
  assert.equal(await ciclo.start(), true)
  const parando = ciclo.stop()
  assert.equal(await ciclo.start(), false, 'sem fila: recusado enquanto drena')
  await parando
  sessao.valor = {}
  assert.equal(await ciclo.start(), true, 'após a drenagem, nova sessão abre geração nova')
  assert.equal(await ciclo.stop(), true)
})

test('linha inválida, linha extra, porta privilegiada, bearer rejeitado e porta que não escuta viram recusa que o prazo de hello invalida', async t => {
  for (const cenario of ['LINHA_INVALIDA', 'LINHA_EXTRA', 'LINHA_ATRASADA', 'PORTA_BAIXA', 'BEARER_REJEITA', 'NAO_ESCUTA']) {
    const { caminho, fd } = filho(t, cenario)
    const { ciclo } = cicloDo({ role: 'hub', path: `filho-${cenario.toLowerCase()}`, fd })
    assert.equal(await ciclo.start(), true, cenario)
    assert.equal(await ciclo.hello(), null, `${cenario}: hello nunca resolve, o prazo decide`)
    assert.equal(await ciclo.stop(), true, `${cenario}: geração invalidada e grupo morto confirmado`)
    assert.ok(await morreu(caminho, 3000), cenario)
  }
})

test('filho que aceita a conexão e nunca responde não deixa órfão quando a geração é abortada', async t => {
  const { caminho, fd } = filho(t, 'MUDO')
  const { ciclo } = cicloDo({ role: 'hub', path: 'filho-mudo', fd })
  assert.equal(await ciclo.start(), false, 'abertura expira no prazo próprio')
  assert.equal(await ciclo.stop(), false, 'partida pendente: recomeço inseguro é recusado')
  assert.ok(await morreu(caminho, 5000), 'o descarte tardio reap o filho após o abort')
})

test('filho que ignora EOF e SIGTERM só sai com o SIGKILL ao grupo', async t => {
  const { caminho, fd } = filho(t, 'IGNORA_SIGTERM')
  const { ciclo } = cicloDo({ role: 'hub', path: 'filho-ignora_sigterm', fd })
  assert.equal(await ciclo.start(), true)
  assert.deepEqual(await ciclo.hello(), { computerId: 'filho-teste', hostId: 'filho-hub' })
  assert.equal(await ciclo.stop(), true, 'escalada completa: EOF e SIGTERM ignorados, SIGKILL confirmou')
  assert.ok(await morreu(caminho, 2000))
})

test('descendente pendurado morre com o grupo, confirmado pela sonda, com o filho direto servindo', async t => {
  const { caminho, fd } = filho(t, 'DESCENDENTE')
  const { ciclo } = cicloDo({ role: 'hub', path: 'filho-descendente', fd })
  assert.equal(await ciclo.start(), true)
  assert.deepEqual(await ciclo.hello(), { computerId: 'filho-teste', hostId: 'filho-hub' })
  assert.equal(await ciclo.stop(), true, 'descendente morto com o grupo, confirmado pela sonda')
  assert.ok(await morreu(caminho, 3000))
})

test('exec que nunca aconteceu é recusa limpa: nada vivo e recomeço permitido', async t => {
  const pasta = mkdtempSync(join(tmpdir(), 'rc2-ruim-'))
  t.after(() => rmSync(pasta, { recursive: true, force: true }))
  const alvoRuim = join(pasta, 'nao-executavel')
  writeFileSync(alvoRuim, 'conteúdo sem permissão de execução')
  chmodSync(alvoRuim, 0o644)
  const { sessao, ciclo } = cicloDo({ role: 'hub', path: 'nao-executavel', fd: openSync(alvoRuim, 'r') })
  assert.equal(await ciclo.start(), false, 'exec recusado antes do pid: nada vivo')
  assert.equal(await ciclo.stop(), true, 'recusa limpa permite recomeço')
  const bom = filho(t, '')
  const transporteBom = rc2NativeTransport(artefato({ role: 'hub', path: 'filho-padrao', fd: bom.fd }))!
  const cicloBom = new Rc2HubLifecycle(() => sessao.valor, transporteBom, { deadlines, schedule: fakeSchedule })
  assert.equal(await cicloBom.start(), true)
  assert.deepEqual(await cicloBom.hello(), { computerId: 'filho-teste', hostId: 'filho-hub' })
  assert.equal(await cicloBom.stop(), true)
  assert.ok(await morreu(bom.caminho, 2000))
})
