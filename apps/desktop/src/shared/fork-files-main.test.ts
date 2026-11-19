// main/files.ts real: leitura de arquivos escolhidos/colados vindos do SO, com electron simulado.
import assert from 'node:assert/strict'
import * as fs from 'node:fs'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { join } from 'node:path'
import test from 'node:test'
import * as url from 'node:url'
import { pathToFileURL } from 'node:url'
import { loadMainModule } from './test-support/main-module.ts'

type Item = { types: string[]; getType: (t: string) => Promise<Blob> }

function carregar(items: Item[]) {
  return loadMainModule<typeof import('../main/files.ts')>('files', {
    electron: { clipboard: { read: async () => items } },
    'node:fs': fs, 'node:path': path, 'node:url': url,
  }, { Uint8Array, Date: { now: () => 1_700_000_123_999 } })
}
const item = (types: string[], conteudo: Record<string, string | Uint8Array>): Item => ({
  types,
  getType: async (t) => new Blob([conteudo[t] as BlobPart]),
})

test('readFiles devolve só o nome base e pula o que não consegue ler', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'mx-files-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  await writeFile(join(dir, 'a.txt'), 'oi')
  const out = await carregar([]).readFiles([join(dir, 'a.txt'), join(dir, 'nao-existe'), dir])
  assert.equal(out.length, 1)
  assert.equal(out[0]!.name, 'a.txt')
  assert.deepEqual([...out[0]!.data], [...Buffer.from('oi')])
})

test('colar arquivo copiado: lê só file://, ignora outros esquemas e remove NUL final', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'mx-files-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  await writeFile(join(dir, 'x.txt'), 'x')
  const lista = ['# comentário', 'https://example.test/y', pathToFileURL(join(dir, 'x.txt')).href + '\0'].join('\r\n')
  const out = await carregar([item(['text/uri-list'], { 'text/uri-list': lista })]).readClipboardFiles()
  assert.equal(JSON.stringify(out.map((o) => o.name)), '["x.txt"]')
})

test('colar imagem: PNG vira pasted-<epoch>.png e JPEG vira .jpg; imagem com texto junto é ignorada', async () => {
  const png = new Uint8Array([137, 80, 78, 71])
  const m = carregar([
    item(['image/png'], { 'image/png': png }),
    item(['image/jpeg'], { 'image/jpeg': new Uint8Array([255, 216]) }),
    item(['image/png', 'text/plain'], { 'image/png': png, 'text/plain': 'nome.png' }),
    item(['image/gif'], { 'image/gif': new Uint8Array([1]) }),
  ])
  const out = await m.readClipboardFiles()
  assert.equal(JSON.stringify(out.map((o) => o.name)), '["pasted-1700000123.png","pasted-1700000123.jpg"]')
  assert.deepEqual([...out[0]!.data], [...png])
})

test('área de transferência vazia ou só com texto não devolve arquivo', async () => {
  assert.equal((await carregar([]).readClipboardFiles()).length, 0)
  assert.equal((await carregar([item(['text/plain'], { 'text/plain': 'oi' })]).readClipboardFiles()).length, 0)
})
