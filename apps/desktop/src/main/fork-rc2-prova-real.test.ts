import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdtempSync, openSync, readdirSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { before, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import type { Rc2Deadlines, Rc2Schedule } from './fork-rc2-local-hub.ts'
import { Rc2HubLifecycle } from './fork-rc2-local-hub.ts'
import { rc2NativeTransport } from './fork-rc2-transport.ts'

// Prova com processo real do item 3 da peça: a morte do pai (sem stop, sem will-quit) tem de
// encerrar o filho pelo EOF que o fechamento dos descritores produz, e o timeout na fase de
// bootstrap — antes de qualquer porta conhecida — tem de ser morto pelo escalonamento com
// evidência de qual sinal foi suficiente. Nada aqui usa fake de transporte.

const deadlines: Rc2Deadlines = { open: 3000, hello: 400, close: 1000, eof: 300, sigterm: 400, sigkill: 400, probe: 20 }
const fakeSchedule: Rc2Schedule = (ms, fire) => {
  const timer = setTimeout(fire, ms)
  return () => clearTimeout(timer)
}

const fonteC = fileURLToPath(new URL('./test-support/rc2-filho-teste.c', import.meta.url))
const paiTs = fileURLToPath(new URL('./test-support/rc2-pai-prova.ts', import.meta.url))
let compiladorAusente = false
const binarios: Record<string, string> = {}

before(() => {
  if (process.platform !== 'linux') return
  const pasta = mkdtempSync(join(tmpdir(), 'rc2-prova-build-'))
  for (const [nome, flags] of [['padrao', []], ['ignora-sigterm', ['-DCENARIO_IGNORA_SIGTERM']], ['sem-linha', ['-DCENARIO_SEM_LINHA']]] as const) {
    const saida = join(pasta, `filho-${nome}.bin`)
    const resultado = spawnSync('cc', ['-O2', '-o', saida, fonteC, ...flags])
    if (resultado.error) { compiladorAusente = true; return }
    if (resultado.status !== 0) throw new Error(`cc falhou para ${nome}: ${resultado.stderr}`)
    binarios[nome] = saida
  }
})

// t.skip() não interrompe o callback do teste: por isso pronto devolve booleano e cada
// teste retorna de verdade quando os pré-requisitos faltam — sem skip, nenhum cenário roda
// com binário inexistente e nenhuma limpeza toca /proc fora do Linux.
function pronto(t: { skip: (motivo: string) => unknown }): boolean {
  if (process.platform !== 'linux') { t.skip('vetor de processo real só existe no Linux: sem base de descritor o transporte é inerte'); return false }
  if (compiladorAusente) { t.skip('sem compilador C neste ambiente: o vetor de processo real não roda'); return false }
  return true
}

// Copia o binário para uma pasta exclusiva da prova: os marcadores caem ao lado dele
// (dirname de /proc/self/exe) e a sonda por exe é exata, sem confundir cenários.
function estagiar(t: { after: (cleanup: () => unknown) => unknown }, nome: string): { pasta: string; caminho: string } {
  const pasta = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-prova-'))
  t.after(() => {
    // Limpeza de melhor esforço: nenhum processo da cópia sobrevive à suíte, nem mutante.
    if (process.platform !== 'linux') { rmSync(pasta, { recursive: true, force: true }); return }
    for (const pid of readdirSync('/proc').filter(entrada => /^\d+$/.test(entrada))) {
      try {
        if (realpathSync(`/proc/${pid}/exe`) === join(pasta, 'filho')) {
          try { process.kill(Number(pid), 'SIGKILL') } catch { /* já morto */ }
        }
      } catch { /* processo já saiu */ }
    }
    rmSync(pasta, { recursive: true, force: true })
  })
  const caminho = join(pasta, 'filho')
  copyFileSync(binarios[nome], caminho)
  return { pasta, caminho }
}

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

async function vivo(caminho: string): Promise<boolean> {
  return !(await morreu(caminho, 0))
}

function cicloDe(fd: number) {
  const sessao = {}
  let partidas = 0
  const interno = rc2NativeTransport({ components: [{ role: 'hub', path: 'filho', fd }], close: () => { try { /* o dono do fd é o teste */ } catch { /* encerrado */ } } })
  assert.ok(interno !== null, 'linux tem base de execução por descritor')
  const ciclo = new Rc2HubLifecycle(() => sessao, signal => { partidas++; return interno(signal) }, { deadlines, schedule: fakeSchedule })
  return { ciclo, partidas: () => partidas }
}

test('morte do pai por SIGKILL, sem stop e sem will-quit, encerra o filho graciosamente pelo EOF dos descritores', async t => {
  if (!pronto(t)) return
  const { pasta, caminho } = estagiar(t, 'padrao')
  const pai = spawn(process.execPath, ['--experimental-strip-types', paiTs, caminho], { detached: true, stdio: ['ignore', 'pipe', 'pipe'] })
  t.after(() => { try { process.kill(-pai.pid!, 'SIGKILL') } catch { /* o grupo do pai já morreu */ } })
  let saida = ''
  pai.stdout!.on('data', (chunk: Buffer) => { saida += chunk.toString('utf8') })
  const fim = Date.now() + 8000
  while (!saida.includes('pronto')) {
    if (Date.now() > fim) assert.fail(`pai nunca confirmou o hello: ${saida}`)
    await new Promise(retoma => setTimeout(retoma, 30))
  }
  assert.ok(await vivo(caminho), 'filho saudável servindo antes da morte do pai')
  process.kill(pai.pid!, 'SIGKILL')
  assert.ok(await morreu(caminho, 5000), 'o EOF dos descritores fechados do pai encerra o filho sozinho')
  const prazo = Date.now() + 2000
  while (!existsSync(join(pasta, 'eof-gracioso.marcador')) && Date.now() < prazo) await new Promise(retoma => setTimeout(retoma, 20))
  assert.ok(existsSync(join(pasta, 'eof-gracioso.marcador')), 'a saída foi a graciosa do EOF, não uma morte por sinal')
})

test('controle: filho que não sai no EOF não grava marcador nenhum quando o pai morre — o focal positivo detecta', async t => {
  if (!pronto(t)) return
  const { pasta, caminho } = estagiar(t, 'ignora-sigterm')
  const pai = spawn(process.execPath, ['--experimental-strip-types', paiTs, caminho], { detached: true, stdio: ['ignore', 'pipe', 'ignore'] })
  t.after(() => { try { process.kill(-pai.pid!, 'SIGKILL') } catch { /* já morto */ } })
  let saida = ''
  pai.stdout!.on('data', (chunk: Buffer) => { saida += chunk.toString('utf8') })
  const fim = Date.now() + 8000
  while (!saida.includes('pronto')) {
    if (Date.now() > fim) assert.fail('pai nunca confirmou o hello no controle')
    await new Promise(retoma => setTimeout(retoma, 30))
  }
  process.kill(pai.pid!, 'SIGKILL')
  // O mutante segue servindo com stdin fechado: nenhum marcador pode aparecer e o processo
  // continua vivo até o observador acabar com ele — sem isso, o focal positivo provaria nada.
  await new Promise(retoma => setTimeout(retoma, 1500))
  assert.ok(!existsSync(join(pasta, 'eof-gracioso.marcador')), 'o mutante nunca grava o marcador gracioso')
  assert.ok(await vivo(caminho), 'o mutante sobrevive ao EOF — a diferença é o comportamento do filho, não o acaso')
})

test('filho que nunca escreve a linha de bootstrap morre pelo SIGTERM do escalonamento, com evidência do sinal', async t => {
  if (!pronto(t)) return
  const { pasta, caminho } = estagiar(t, 'sem-linha')
  const { ciclo, partidas } = cicloDe(openSync(caminho, 'r'))
  t.after(async () => { try { await ciclo.stop() } catch { /* melhor esforço */ } })
  assert.equal(await ciclo.start(), false, 'sem linha de bootstrap a abertura expira no prazo próprio')
  assert.equal(await ciclo.stop(), false, 'partida pendente: recomeço inseguro é recusado')
  const prazo = Date.now() + 5000
  while (!existsSync(join(pasta, 'sigterm.marcador')) && Date.now() < prazo) await new Promise(retoma => setTimeout(retoma, 20))
  assert.ok(existsSync(join(pasta, 'sigterm.marcador')), 'o SIGTERM chegou e foi suficiente — o EOF era inútil, o SIGKILL não foi preciso')
  assert.ok(await morreu(caminho, 3000), 'processo morto confirmado pela sonda externa')
  const aberturas = partidas()
  assert.equal(await ciclo.start(), false, 'a partida expirada deixa o ciclo inseguro: recomeço recusado')
  assert.equal(partidas(), aberturas, 'o recomeço recusado não executa processo novo nenhum — não é só o retorno falso')
})

