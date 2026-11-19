import { readdirSync, readFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join } from 'node:path'
import { newSSHProfile, type SSHProfile } from '../../shared/ssh.ts'

// D76/D293: o perfil SSH do Hub só nasce se o alias existir no ~/.ssh/config desta máquina.
// Sem alias, nada é criado nem gravado, e nenhuma conexão é tentada.

/** Linhas `Host` e `Include` de um texto de ssh_config (palavra-chave sem caixa, `=` opcional, aspas). */
export function sshConfigEntries(text: string): { hosts: string[]; includes: string[] } {
  const hosts: string[] = []
  const includes: string[] = []
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const m = /^(\S+?)(?:\s*=\s*|\s+)(.*)$/.exec(line)
    if (!m) continue
    const tokens = [...m[2].matchAll(/"([^"]*)"|(\S+)/g)].map((t) => t[1] ?? t[2])
    const key = m[1].toLowerCase()
    if (key === 'host') hosts.push(...tokens)
    else if (key === 'include') includes.push(...tokens)
  }
  return { hosts, includes }
}

/** O alias aparece como `Host` exato (sem curinga nem negação). */
export const sshConfigHasHost = (text: string, alias: string) => sshConfigEntries(text).hosts.includes(alias)

/** Lê o config e seus `Include` (caminho fixo ou curinga só no nome do arquivo), até 4 níveis. */
export function sshAliasExists(alias: string, home: string, file = join(home, '.ssh', 'config'), depth = 0): boolean {
  let text: string
  try { text = readFileSync(file, 'utf8') } catch { return false }
  const { hosts, includes } = sshConfigEntries(text)
  if (hosts.includes(alias)) return true
  if (depth >= 4) return false
  for (const inc of includes) {
    const path = inc.startsWith('~/') ? join(home, inc.slice(2)) : isAbsolute(inc) ? inc : join(home, '.ssh', inc)
    const dir = dirname(path)
    const name = basename(path)
    if (/[*?]/.test(dir)) continue // curinga em pasta: não seguido (D293)
    let files = [path]
    if (/[*?]/.test(name)) {
      const re = new RegExp(`^${name.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.')}$`)
      try { files = readdirSync(dir).filter((f) => re.test(f)).sort().map((f) => join(dir, f)) } catch { files = [] }
    }
    if (files.some((f) => sshAliasExists(alias, home, f, depth + 1))) return true
  }
  return false
}

/** Perfis da primeira abertura, ou null quando nada deve ser criado nem gravado. */
export function seedProfiles(o: { main: boolean; profilesFileExists: boolean; aliasExists: () => boolean; alias: string; port: number }): SSHProfile[] | null {
  if (!o.main || o.profilesFileExists || !o.aliasExists()) return null
  return [{ ...newSSHProfile(o.port), host: o.alias }]
}
