// Downloads compartilhados por bots na web local: o renderer pede os mesmos `begin`/`write`/
// `finish`/`cancel` do desktop (CodyncBridge['files']); aqui os pedaços viram um Blob e o
// navegador salva com `<a download>`. Os limites são os do desktop (`shared/files.ts`:
// 100 MiB no máximo, nome sanitizado) e tudo é validado antes de virar arquivo (fail-closed).
// A ação de salvar entra por `baixar` (seam injetável): os testes rodam em Node sem navegador.

import type { SharedFile } from '../shared/models.ts'
import { saveName, validateSharedFile } from '../shared/files.ts'

export interface OpcoesArquivosWeb {
  /** Recebe o nome de arquivo e o Blob pronto; o padrão dispara o download do navegador. */
  baixar?: (nome: string, blob: Blob) => void
}

/** No máximo 16 downloads abertos ao mesmo tempo: mais que isso é o renderer engasgado, não um usuário. */
const LIMITE_ATIVOS = 16

interface Download {
  nome: string
  tamanho: number
  sha256: string
  pedacos: Map<number, Uint8Array>
  recebido: number
}

function novoId(): string {
  return typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`
}

/** O download padrão do navegador: <a download> com URL de objeto revogada na sequência. */
function baixarPadrao(nome: string, blob: Blob) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = nome
  a.rel = 'noopener'
  document.body.append(a)
  a.click()
  a.remove()
  // Revogar na hora mata a ObjectURL antes de o Chromium começar o download; 10 s dá folga
  // para a cópia pegar (Chromium às vezes lê a URL de forma adiada) e .unref não segura o processo.
  const revogar = setTimeout(() => URL.revokeObjectURL(url), 10_000)
  ;(revogar as { unref?: () => void }).unref?.()
}

/** Pedaços em ordem de offset, sem buraco e sem sobra: um arquivo fechado em um único buffer. */
function montar(download: Download): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(download.tamanho)
  let esperado = 0
  for (const [offset, pedaco] of [...download.pedacos].sort((a, b) => a[0] - b[0])) {
    // Offset menor que o esperado é sobreposição; maior, buraco; e nada pode passar do fim do arquivo.
    if (offset !== esperado) throw new Error('incomplete download')
    bytes.set(pedaco, offset)
    esperado += pedaco.length
  }
  if (esperado !== download.tamanho) throw new Error('incomplete download')
  return bytes
}

/** O resumo em hex minúsculo (mesma forma do `sha256` validado no `begin`), para comparar byte a byte. */
async function resumo(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const cripto = globalThis.crypto?.subtle
  if (!cripto) throw new Error('integrity check unavailable')
  const digest = new Uint8Array(await cripto.digest('SHA-256', bytes))
  return [...digest].map((b) => b.toString(16).padStart(2, '0')).join('')
}

export function createLocalFiles(opts: OpcoesArquivosWeb = {}): Pick<import('../shared/ipc.ts').CodyncBridge, 'files'>['files'] {
  const baixar = opts.baixar ?? baixarPadrao
  const ativos = new Map<string, Download>()

  return {
    async begin(file: SharedFile) {
      validateSharedFile(file)
      if (ativos.size >= LIMITE_ATIVOS) throw new Error('too many downloads')
      const id = novoId()
      ativos.set(id, { nome: saveName(file.name), tamanho: file.size, sha256: file.sha256, pedacos: new Map(), recebido: 0 })
      return id
    },

    async write(id, offset, data) {
      const download = ativos.get(id)
      if (!download) throw new Error('unknown download')
      if (!Number.isSafeInteger(offset) || offset < 0) throw new Error('invalid offset')
      if (!(data instanceof Uint8Array) || data.length === 0) throw new Error('invalid chunk')
      if (offset + data.length > download.tamanho) throw new Error('download overflow')
      // Offsets sobrepostos caberiam um a um no tamanho do arquivo: o total recebido é o freio.
      if (download.recebido + data.length > download.tamanho) throw new Error('download overflow')
      if (download.pedacos.has(offset)) throw new Error('duplicate chunk')
      download.pedacos.set(offset, data)
      download.recebido += data.length
    },

    async finish(id) {
      const download = ativos.get(id)
      if (!download) throw new Error('unknown download')
      ativos.delete(id)
      const bytes = montar(download)
      // O resumo anunciado no `begin` decide: sem a conferência, o arquivo não chega ao usuário.
      if (await resumo(bytes) !== download.sha256) throw new Error('integrity mismatch')
      await baixar(download.nome, new Blob([bytes]))
    },

    async cancel(id) {
      ativos.delete(id)
    },
  }
}
