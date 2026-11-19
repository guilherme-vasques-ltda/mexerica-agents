import { execFile } from 'node:child_process'
import { lstatSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { environmentProfile } from '../../shared/environment'
import { pacotesDoApp } from '../../shared/fork-nome-pacote'
import { screenAgentRawStatus } from '../screen'

// D331 C: a caixa não abre, ou fecha, num Mac com o ajudante de tela instalado, registrado ou rodando.
// Confere as duas variantes oficiais (main e Dev), que observam o mesmo desktop. Só três respostas:
// presente, ausente comprovado ou incerto; erro, dado ausente ou consulta impossível contam como incerto.
type Fact = 'sim' | 'não' | 'incerto'
const variants = (['main', 'dev'] as const).map(environmentProfile)

/** Comando cujo código `absent` comprova ausência; 0 é presença; o resto é incerto. */
function probe(bin: string, args: string[], absent: number): Promise<Fact> {
  return new Promise(resolve => {
    execFile(bin, args, { timeout: 2000, windowsHide: true }, error => {
      if (!error) return resolve('sim')
      resolve(error.code === absent && !error.killed ? 'não' : 'incerto')
    })
  })
}

/**
 * Binário do ajudante de cada variante em cada bundle conhecido: este app e o `<nome>.app` da variante
 * em /Applications e ~/Applications (locais de instalação do app e dos guias), com o nome novo e o antigo do
 * pacote (D443, shared/fork-nome-pacote). Só ENOENT/ENOTDIR são ausência.
 */
function installed(name: string): Fact {
  const contents = [join(process.resourcesPath, '..'), ...variants.flatMap(v => pacotesDoApp(v.environment).flatMap(nome => [
    `/Applications/${nome}.app/Contents`, join(homedir(), 'Applications', `${nome}.app`, 'Contents'),
  ]))]
  let fact: Fact = 'não'
  for (const dir of contents) {
    try {
      lstatSync(join(dir, 'Library/LoginItems', `${name}.app/Contents/MacOS/${name}`))
      return 'sim'
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code
      if (code !== 'ENOENT' && code !== 'ENOTDIR') fact = 'incerto'
    }
  }
  return fact
}

/** Login item deste app, sem o fallback do gerenciamento comum: só `not-registered`/`not-found` são ausência. */
function ownAgent(): Fact {
  try {
    const status = screenAgentRawStatus()
    if (status === null || status === undefined) return 'incerto'
    return status === 'not-registered' || status === 'not-found' ? 'não' : 'sim'
  } catch {
    return 'incerto'
  }
}

/** Sinais de observador neste Mac, conferidos agora. Vazio só com tudo conferido e ausente. */
export async function observerSignals(): Promise<string[]> {
  try {
    const uid = process.getuid?.()
    // Ordem: processo, depois launchd, depois os fatos síncronos (bundle, login item), sem espera depois.
    // Registrar exige o bundle instalado, que a leitura final enxerga.
    const running = await Promise.all(variants.map(v => probe('/usr/bin/pgrep', ['-f', '--', `/${v.screenName}.app/Contents/MacOS/${v.screenName}`], 1)))
    // launchctl: 113 = serviço não encontrado no domínio do usuário.
    const loaded = await Promise.all(variants.map(v => uid === undefined ? 'incerto' as Fact : probe('/bin/launchctl', ['print', `gui/${uid}/${v.screenLabel}`], 113)))
    const signals: string[] = []
    const own = ownAgent()
    if (own !== 'não') signals.push(`agente:${own}`)
    variants.forEach((v, i) => {
      const binary = installed(v.screenName)
      if (binary !== 'não') signals.push(`${v.screenName}:binário:${binary}`)
      if (loaded[i] !== 'não') signals.push(`${v.screenName}:launchd:${loaded[i]}`)
      if (running[i] !== 'não') signals.push(`${v.screenName}:processo:${running[i]}`)
    })
    return signals
  } catch {
    return ['conferência']
  }
}
