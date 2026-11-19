// Fronteira de segurança: download de arquivo no main process (files:begin/write/finish/cancel).
// Usa o main/file-saves.ts e o main/file-sink.ts reais, com electron simulado e disco temporário.
import assert from 'node:assert/strict'
import * as crypto from 'node:crypto'
import { createHash } from 'node:crypto'
import * as fsPromises from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

type Handler = (...args: any[]) => Promise<any>

async function fixture(t: test.TestContext) {
  const dir = await mkdtemp(join(tmpdir(), 'mx-saves-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  const handlers = new Map<string, Handler>()
  const dialogos: { defaultPath: string }[] = []
  let escolha: string | null = join(dir, 'saida.bin')
  const electron = {
    ipcMain: { handle: (nome: string, fn: Handler) => handlers.set(nome, fn) },
    BrowserWindow: { fromWebContents: () => ({}) },
    dialog: { showSaveDialog: async (_w: unknown, opts: { defaultPath: string }) => { dialogos.push(opts); return escolha ? { canceled: false, filePath: escolha } : { canceled: true } } },
  }
  const mod = loadMainModule<typeof import('../main/file-saves.ts')>('file-saves', {
    electron, 'node:crypto': crypto, 'node:fs/promises': fsPromises,
  }, {})
  mod.registerFileSaves()
  const remetente = (id: number) => {
    const ouvintes = new Map<string, () => void>()
    return {
      id, destruido: false, isDestroyed() { return this.destruido },
      once: (n: string, fn: () => void) => ouvintes.set(n, fn), removeListener: (n: string) => ouvintes.delete(n),
      destruir() { this.destruido = true; ouvintes.get('destroyed')?.() },
    }
  }
  const chamar = (nome: string, sender: ReturnType<typeof remetente>, ...args: unknown[]) => handlers.get(nome)!({ sender }, ...args)
  return { dir, chamar, remetente, dialogos, escolher: (p: string | null) => { escolha = p } }
}

const meta = (bytes: Uint8Array, name = 'a.bin') => ({ name, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), mime: 'application/octet-stream' })

test('download completo: grava só no destino, com bytes e hash íntegros, sem sobra de parcial', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  const bytes = new Uint8Array([1, 2, 3, 4, 5])
  const id = await f.chamar('files:begin', dono, meta(bytes))
  await f.chamar('files:write', dono, id, 0, bytes)
  await f.chamar('files:finish', dono, id)
  assert.deepEqual([...(await readFile(join(f.dir, 'saida.bin')))], [...bytes])
  assert.deepEqual(await readdir(f.dir), ['saida.bin'])
})

test('metadados inválidos são recusados antes de abrir o diálogo de salvar', async (t) => {
  const f = await fixture(t)
  const ok = meta(new Uint8Array([1]))
  for (const ruim of [
    { ...ok, name: '../x' }, { ...ok, name: 'a/b' }, { ...ok, name: '..' }, { ...ok, name: '' }, { ...ok, name: 'a\0b' },
    { ...ok, sha256: 'xyz' }, { ...ok, size: -1 }, { ...ok, size: 1.5 }, { ...ok, size: 101 * 1024 * 1024 },
  ]) await assert.rejects(f.chamar('files:begin', f.remetente(1), ruim), Error, JSON.stringify(ruim).slice(0, 60))
  assert.equal(f.dialogos.length, 0)
})

test('o nome sugerido ao diálogo nunca é um caminho', async (t) => {
  const f = await fixture(t)
  f.escolher(null)
  await f.chamar('files:begin', f.remetente(1), meta(new Uint8Array([1]), 'con.txt'))
  assert.ok(!/[/\\]/.test(f.dialogos[0]!.defaultPath))
  assert.notEqual(f.dialogos[0]!.defaultPath, 'con.txt')
})

test('diálogo cancelado: devolve null e não cria arquivo', async (t) => {
  const f = await fixture(t)
  f.escolher(null)
  assert.equal(await f.chamar('files:begin', f.remetente(1), meta(new Uint8Array([1]))), null)
  assert.deepEqual(await readdir(f.dir), [])
})

test('outro remetente não escreve, finaliza nem cancela o download alheio, e o dono segue podendo concluir', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  const intruso = f.remetente(2)
  const bytes = new Uint8Array([9, 8, 7])
  const id = await f.chamar('files:begin', dono, meta(bytes))
  await assert.rejects(f.chamar('files:write', intruso, id, 0, bytes), /Unknown download/)
  await assert.rejects(f.chamar('files:finish', intruso, id), /Unknown download/)
  await assert.rejects(f.chamar('files:cancel', intruso, id), /Unknown download/)
  await f.chamar('files:write', dono, id, 0, bytes)
  await f.chamar('files:finish', dono, id)
  assert.deepEqual([...(await readFile(join(f.dir, 'saida.bin')))], [...bytes])
})

test('id desconhecido: write e finish recusam; cancel é silencioso', async (t) => {
  const f = await fixture(t)
  const s = f.remetente(1)
  await assert.rejects(f.chamar('files:write', s, 'nao-existe', 0, new Uint8Array([1])), /Unknown download/)
  await assert.rejects(f.chamar('files:finish', s, 'nao-existe'), /Unknown download/)
  assert.equal(await f.chamar('files:cancel', s, 'nao-existe'), undefined)
})

test('hash errado: finish recusa, o destino não existe e o parcial é removido', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  const real = new Uint8Array([1, 2, 3])
  const id = await f.chamar('files:begin', dono, meta(real))
  await f.chamar('files:write', dono, id, 0, new Uint8Array([1, 2, 4]))
  await assert.rejects(f.chamar('files:finish', dono, id), /integrity/)
  assert.equal(existsSync(join(f.dir, 'saida.bin')), false)
  assert.deepEqual(await readdir(f.dir), [])
  await assert.rejects(f.chamar('files:write', dono, id, 0, real), /Unknown download/)
})

test('pedaço fora de ordem ou com tamanho errado cancela o download e limpa o parcial', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  const bytes = new Uint8Array([1, 2, 3, 4])
  const id = await f.chamar('files:begin', dono, meta(bytes))
  await assert.rejects(f.chamar('files:write', dono, id, 2, bytes.slice(2)), /Invalid file chunk/)
  assert.deepEqual(await readdir(f.dir), [])
  assert.equal(existsSync(join(f.dir, 'saida.bin')), false)
})

test('janela destruída no meio: o download é cancelado e o parcial removido', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  const bytes = new Uint8Array([1, 2, 3])
  const id = await f.chamar('files:begin', dono, meta(bytes))
  assert.equal((await readdir(f.dir)).length, 1)
  dono.destruir()
  for (let i = 0; i < 100 && (await readdir(f.dir)).length > 0; i++) await new Promise((r) => setTimeout(r, 20))
  assert.deepEqual(await readdir(f.dir), [])
  await assert.rejects(f.chamar('files:write', f.remetente(1), id, 0, bytes), /Unknown download/)
})

test('janela destruída enquanto o diálogo estava aberto: nada é criado', async (t) => {
  const f = await fixture(t)
  const dono = f.remetente(1)
  dono.destruido = true
  assert.equal(await f.chamar('files:begin', dono, meta(new Uint8Array([1]))), null)
  assert.deepEqual(await readdir(f.dir), [])
})
