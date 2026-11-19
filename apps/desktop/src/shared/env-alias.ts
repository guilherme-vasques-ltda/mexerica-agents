/**
 * Lê `MEXERICA_*` primeiro e cai para o nome antigo (alias por 2 releases, issue #915).
 * Valor vazio conta como ausente. O nome antigo avisa uma vez por variável e continua valendo.
 * Só para configuração de runtime (porta, binário, URL); nunca para nomes de cofre/userData.
 */
const warned = new Set<string>()

export function envAlias(env: Record<string, string | undefined>, name: string, legacy: string, warn: (message: string) => void = console.warn): string | undefined {
  const current = env[name]
  if (current) return current
  const old = env[legacy]
  if (!old) return undefined
  if (!warned.has(legacy)) {
    warned.add(legacy)
    warn(`[mexerica] ${legacy} está obsoleta e sai em 2 releases; use ${name}.`)
  }
  return old
}
