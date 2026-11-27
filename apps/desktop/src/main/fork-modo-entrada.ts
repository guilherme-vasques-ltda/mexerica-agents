import { closeSync, constants, fchmodSync, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { escolhaEntradaValida, type EscolhaEntrada } from '../shared/fork-modo-entrada.ts'

const posix = process.platform !== 'win32'

/** O arquivo da escolha "100% local" ou "com conta", dentro de `userData`, só do dono (0600). */
export function arquivoEscolhaEntrada(userDataDir: string): string {
  return join(userDataDir, 'escolha-entrada.json')
}

export function lerEscolhaEntrada(arquivo: string): EscolhaEntrada | null {
  try {
    const fd = openSync(arquivo, constants.O_RDONLY | (posix ? constants.O_NOFOLLOW : 0))
    try {
      if (!fstatSync(fd).isFile()) return null
      return escolhaEntradaValida(JSON.parse(readFileSync(fd, 'utf8')))
    } finally {
      closeSync(fd)
    }
  } catch {
    return null
  }
}

export function gravarEscolhaEntrada(arquivo: string, valor: unknown): boolean {
  const escolha = escolhaEntradaValida(valor)
  if (!escolha) return false
  try {
    mkdirSync(dirname(arquivo), { recursive: true, mode: 0o700 })
    try {
      if (!lstatSync(arquivo).isFile()) return false
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') return false
    }
    const temporario = `${arquivo}.${process.pid}.tmp`
    try {
      const fd = openSync(temporario, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (posix ? constants.O_NOFOLLOW : 0), 0o600)
      try {
        if (posix) fchmodSync(fd, 0o600)
        writeFileSync(fd, JSON.stringify(escolha))
        fsyncSync(fd)
      } finally {
        closeSync(fd)
      }
      renameSync(temporario, arquivo)
      return true
    } catch {
      rmSync(temporario, { force: true })
      return false
    }
  } catch {
    return false
  }
}
