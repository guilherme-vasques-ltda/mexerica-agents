// Hub local do Mac ("Neste Mac", plano docs/plans/mac-local-hub.md, fatia P). Regras puras do
// corte: a chave de build `macLocalHub` e a escolha do usuário `hubLocation` valem juntas, e a
// escolha é lida a cada chamada (nunca fixada quando o módulo carrega).

export type HubLocation = 'mac' | 'remote'

/** Preferência do usuário; qualquer valor desconhecido ou ausente é "remoto" (estado `disabled`). */
export function hubLocation(read: (key: string) => unknown): HubLocation {
  return read('hubLocation') === 'mac' ? 'mac' : 'remote'
}

export interface MacLocalHubInputs {
  /** `forkFeatures.macLocalHub`: constante de build. */
  feature: boolean
  isMac: boolean
  /** Host de desenvolvimento iniciado à mão (`MEXERICA_PORT`). */
  devPort: number | null
  /** Lê a preferência atual do usuário. */
  read: (key: string) => unknown
}

/** O app não administra um Hub neste computador (estado `disabled`). */
export function localHostOff({ feature, isMac, devPort, read }: MacLocalHubInputs): boolean {
  const macLocal = feature && isMac && hubLocation(read) === 'mac'
  return !macLocal && devPort === null
}

/** Níveis de desinstalação do Hub deste Mac: 3 (apagar tudo) não faz parte da fatia P. */
export type UninstallLevel = 1 | 2

export function uninstallArgs(level: UninstallLevel, windows = false): string[] {
  // Windows segue o caminho herdado (restaura a status line do Claude Code); os níveis são do Mac/Linux.
  return windows ? ['uninstall'] : ['uninstall', '--nivel', String(level)]
}

/** Espera entre reinícios do `KeepAlive` (espelha `mac_local::THROTTLE_SECONDS`). [A medir] num Mac real. */
export const THROTTLE_SECONDS = 30

/** Argumentos que o plist do Hub local do Mac tem de ter (espelha `mac_local::plist` no Hub). */
export function expectedProgramArguments(exe: string, port: number): string[] {
  return [exe, 'serve', '--port', String(port), '--bind', '127.0.0.1', '--only-this-mac']
}

const xmlUnescape = (s: string) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')

/** `ProgramArguments` do plist, na ordem; `null` quando o texto não tem a lista. */
export function parseProgramArguments(plist: string): string[] | null {
  const block = /<key>ProgramArguments<\/key>\s*<array>([\s\S]*?)<\/array>/.exec(plist)
  if (!block) return null
  return [...block[1]!.matchAll(/<string>([\s\S]*?)<\/string>/g)].map((m) => xmlUnescape(m[1]!))
}

/**
 * O plist instalado é o do modo "só neste Mac"? Compara a lista inteira (caminho do binário
 * incluído, em qualquer uma das formas dadas) e não só o caminho: um plist herdado com o mesmo
 * binário e sem `--bind 127.0.0.1 --only-this-mac` não vale e é reinstalado.
 */
export function macPlistValid(plist: string | null, exes: string[], port: number): boolean {
  if (plist === null) return false
  const args = parseProgramArguments(plist)
  if (!args) return false
  const throttle = /<key>ThrottleInterval<\/key>\s*<integer>\s*(\d+)\s*<\/integer>/.exec(plist)
  if (!throttle || Number(throttle[1]) !== THROTTLE_SECONDS) return false
  return exes.some((exe) => {
    const expected = expectedProgramArguments(exe, port)
    return args.length === expected.length && args.every((arg, i) => arg === expected[i])
  })
}

/** `launchctl print gui/<uid>/<label>` falhou por ausência do serviço (não por outro motivo). */
export function launchdServiceAbsent(status: number, output: string): boolean {
  return status !== 0 && (status === 113 || /could not find (the )?(specified )?service/i.test(output))
}
