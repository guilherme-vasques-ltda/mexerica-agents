import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
/** D520-B1. Sem openat no Node: o pai ainda pode ser trocado entre lstat e open.
 * Pai próprio 0700, O_NOFOLLOW e validação pelo fd mitigam; Segurança deve aceitar esse limite.
 */
import { constants, closeSync, fchmodSync, fstatSync, fsyncSync, ftruncateSync, lstatSync, mkdirSync, openSync, writeFileSync, type Stats } from 'node:fs'
import { dirname, parse, resolve, sep } from 'node:path'

function uid() {
  if (!process.getuid) throw new Error(traduzir('pt-BR', 'marca.dados.uidUnix'))
  return process.getuid()
}
export function validatePrivateFile(meta: Stats, owner = uid()) {
  if (!meta.isFile() || meta.uid !== owner || meta.nlink !== 1 || (meta.mode & 0o7777) !== 0o600) {
    throw new Error(traduzir('pt-BR', 'marca.dados.arquivoMetadados'))
  }
}
export function privateDirectory(path: string, create = true) {
  const full = resolve(path)
  const root = parse(full).root
  if (full === root) throw new Error(traduzir('pt-BR', 'marca.dados.raiz'))
  let current = root
  const parts = full.slice(root.length).split(sep)
  for (const [index, part] of parts.entries()) {
    current = resolve(current, part)
    let created = false
    try { lstatSync(current) } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      if (!create) return full
      mkdirSync(current, { mode: 0o700 })
      created = true
    }
    const before = lstatSync(current)
    if (!before.isDirectory() || before.isSymbolicLink() || (before.uid !== 0 && before.uid !== uid())) throw new Error(traduzir('pt-BR', 'marca.dados.paiInvalido'))
    // Ancestrais sticky root permitem apenas fixtures em diretórios temporários sintéticos.
    if ((before.mode & 0o022) && !(before.uid === 0 && (before.mode & 0o1000))) throw new Error(traduzir('pt-BR', 'marca.dados.paiGravavel'))
    const fd = openSync(current, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW)
    try {
      const meta = fstatSync(fd)
      if (!meta.isDirectory() || meta.dev !== before.dev || meta.ino !== before.ino || meta.uid !== before.uid) throw new Error(traduzir('pt-BR', 'marca.dados.paiMudou'))
      if (created) fchmodSync(fd, 0o700)
      const checked = fstatSync(fd)
      if ((created || index === parts.length - 1) && (checked.uid !== uid() || (checked.mode & 0o7777) !== 0o700)) throw new Error(traduzir('pt-BR', 'marca.dados.pastaModo'))
    } finally { closeSync(fd) }
  }
  return full
}
export function openPrivateFile(path: string, append = false) {
  privateDirectory(dirname(path))
  let created = false
  let fd: number
  try {
    fd = openSync(path, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW, 0o600)
    created = true
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
    fd = openSync(path, constants.O_RDWR | constants.O_NOFOLLOW | constants.O_NONBLOCK | (append ? constants.O_APPEND : 0))
  }
  try {
    const before = fstatSync(fd)
    if (!before.isFile() || before.uid !== uid() || before.nlink !== 1) throw new Error(traduzir('pt-BR', 'marca.dados.arquivoInvalido'))
    if (created) fchmodSync(fd, 0o600)
    validatePrivateFile(fstatSync(fd))
    return fd
  } catch (error) { closeSync(fd); throw error }
}
export function writePrivateFile(path: string, bytes: string | Uint8Array, append = false) {
  const fd = openPrivateFile(path, append)
  try {
    if (!append) ftruncateSync(fd, 0)
    writeFileSync(fd, bytes)
    fsyncSync(fd)
  } finally { closeSync(fd) }
}
