import assert from 'node:assert/strict'
import test from 'node:test'
import { createLocalFiles } from './local-files.ts'

const SHA = 'a'.repeat(64)
const arquivo = (tamanho: number): { id: string; name: string; size: number; sha256: string } => ({ id: 'f1', name: 'relatorio.pdf', size: tamanho, sha256: SHA })

/** O resumo real (hex minúsculo) do que vai ser escrito: o mesmo cálculo do `finish`. */
async function resumoDe(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))
  return [...digest].map((b) => b.toString(16).padStart(2, '0')).join('')
}

function criar() {
  const baixados: { nome: string; blob: Blob }[] = []
  const files = createLocalFiles({ baixar: async (nome, blob) => { baixados.push({ nome, blob }) } })
  return { files, baixados }
}

test('begin recusa metadados inválidos (fail-closed)', async () => {
  const { files } = criar()
  await assert.rejects(files.begin({ ...arquivo(10), size: 101 * 1024 * 1024 }))
  await assert.rejects(files.begin({ ...arquivo(10), size: -1 }))
  await assert.rejects(files.begin({ ...arquivo(10), name: 'a/b' }))
  await assert.rejects(files.begin({ ...arquivo(10), sha256: 'xyz' }))
})

test('begin aceita arquivo válido e devolve um id', async () => {
  const { files } = criar()
  const id = await files.begin({ ...arquivo(5), sha256: await resumoDe(new Uint8Array(5)) })
  assert.equal(typeof id, 'string')
  assert.ok(id!.length > 0)
})

test('write recusa id desconhecido, offset inválido e pedaço que passa do tamanho', async () => {
  const { files } = criar()
  const id = await files.begin({ ...arquivo(5), sha256: await resumoDe(new Uint8Array(5)) })
  await assert.rejects(files.write('outro', 0, new Uint8Array([1])))
  await assert.rejects(files.write(id!, -1, new Uint8Array([1])))
  await assert.rejects(files.write(id!, 1.5, new Uint8Array([1])))
  await assert.rejects(files.write(id!, 0, new Uint8Array(6)))
  await assert.rejects(files.write(id!, 0, new Uint8Array(0)))
})

test('write recusa offset repetido e finish recusa download com buraco', async () => {
  const { files } = criar()
  const id = await files.begin({ ...arquivo(6), sha256: await resumoDe(new Uint8Array(6)) })
  await files.write(id!, 3, new Uint8Array([4, 5, 6]))
  await assert.rejects(files.write(id!, 3, new Uint8Array([9])))
  await assert.rejects(files.finish(id!))
})

test('finish monta o Blob na ordem e dispara o download com o nome sanitizado', async () => {
  const { files, baixados } = criar()
  const conteudo = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9])
  const id = await files.begin({ ...arquivo(9), name: 'meu<rel>:atorio*.pdf', sha256: await resumoDe(conteudo) })
  await files.write(id!, 6, new Uint8Array([7, 8, 9]))
  await files.write(id!, 0, new Uint8Array([1, 2, 3]))
  await files.write(id!, 3, new Uint8Array([4, 5, 6]))
  await files.finish(id!)
  assert.equal(baixados.length, 1)
  assert.equal(baixados[0]!.nome, 'meu_rel__atorio_.pdf')
  const bytes = new Uint8Array(await baixados[0]!.blob.arrayBuffer())
  assert.deepEqual([...bytes], [...conteudo])
  assert.equal(baixados[0]!.blob.size, 9)
})

test('arquivo vazio termina limpo e cancel depois de finish é no-op', async () => {
  const { files, baixados } = criar()
  const id = await files.begin({ ...arquivo(0), sha256: await resumoDe(new Uint8Array(0)) })
  await files.finish(id!)
  assert.equal(baixados.length, 1)
  assert.equal(baixados[0]!.blob.size, 0)
  await files.cancel(id!)
})

test('cancel esquece o download: write e finish depois dele recusam', async () => {
  const { files, baixados } = criar()
  const id = await files.begin({ ...arquivo(3), sha256: await resumoDe(new Uint8Array(3)) })
  await files.write(id!, 0, new Uint8Array([1]))
  await files.cancel(id!)
  await assert.rejects(files.write(id!, 1, new Uint8Array([2])))
  await assert.rejects(files.finish(id!))
  assert.equal(baixados.length, 0)
})

test('write recusa pedaços sobrepostos: o total recebido não passa do tamanho', async () => {
  const { files } = criar()
  const id = await files.begin({ ...arquivo(6), sha256: await resumoDe(new Uint8Array(6)) })
  await files.write(id!, 0, new Uint8Array([1, 2, 3, 4]))
  // offset 3 cabe sozinho no arquivo de 6 bytes, mas junto com o que já chegou seria 7.
  await assert.rejects(files.write(id!, 3, new Uint8Array([5, 6, 7])), /download overflow/)
})

test('finish recusa conteúdo com o tamanho certo e resumo errado, sem baixar nada', async () => {
  const { files, baixados } = criar()
  // Tamanho exato, sem buraco: só o resumo errado (hex válido) pode recusar.
  const id = await files.begin({ ...arquivo(5), sha256: 'b'.repeat(64) })
  await files.write(id!, 0, new Uint8Array([1, 2, 3, 4, 5]))
  await assert.rejects(files.finish(id!), /integrity mismatch/)
  assert.equal(baixados.length, 0)
})

test('begin recusa mais de 16 downloads abertos ao mesmo tempo', async () => {
  const { files } = criar()
  const ids: string[] = []
  for (let i = 0; i < 16; i++) {
    const id = await files.begin({ ...arquivo(1), sha256: await resumoDe(new Uint8Array(1)) })
    ids.push(id!)
    await files.write(id!, 0, new Uint8Array([1]))
  }
  await assert.rejects(files.begin({ ...arquivo(1), sha256: await resumoDe(new Uint8Array(1)) }), /too many downloads/)
  // cancel libera a vaga: o próximo download entra no lugar do que saiu.
  await files.cancel(ids[0]!)
  const depois = await files.begin({ ...arquivo(1), sha256: await resumoDe(new Uint8Array(1)) })
  assert.ok(depois)
})
