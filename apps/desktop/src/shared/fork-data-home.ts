import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { isAbsolute, relative, resolve, sep } from 'node:path'

/** Seleção pura; nenhuma leitura de pasta antiga. A presença das variáveis antigas sempre recusa. */
export function mexericaHome(home: string, folder: string, env: Record<string, string | undefined>) {
  if (env.DONNABOT_HOME !== undefined || env.CODYNC_HOME !== undefined) {
    throw new Error(traduzir('pt-BR', 'marca.dados.legado'))
  }
  if (env.MEXERICA_HOME === '') throw new Error(traduzir('pt-BR', 'marca.dados.vazio'))
  const target = resolve(env.MEXERICA_HOME ?? resolve(home, folder))
  const homeRelativeToTarget = relative(target.toLowerCase(), resolve(home).toLowerCase())
  if (!homeRelativeToTarget || (homeRelativeToTarget !== '..' && !homeRelativeToTarget.startsWith(`..${sep}`) && !isAbsolute(homeRelativeToTarget))) {
    throw new Error(traduzir('pt-BR', 'marca.dados.ancestral'))
  }
  const first = relative(resolve(home).toLowerCase(), target.toLowerCase()).split(/[\\/]/)[0]?.toLowerCase()
  if (first?.startsWith('.donnabot') || first?.startsWith('.codync')) throw new Error(traduzir('pt-BR', 'marca.dados.upstream'))
  return target
}
