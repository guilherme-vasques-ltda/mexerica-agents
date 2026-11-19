// MEX-1307 F0 (plano §4 e §4.4): montagem fixa dos argumentos do `ssh` do instalador remoto. Irmão de
// main/ssh-command.ts, no mesmo estilo: vetor de argumentos (nunca `sh -c` com texto do usuário), `--` antes do destino
// e comandos remotos constantes. Do usuário só entram valores validados (host, usuário, porta, arquivo de chave), cada um
// num elemento próprio do vetor. **Senha nunca entra aqui**: nem em argv, nem em ambiente, nem em URL, nem em arquivo.
// A senha do `sudo` segue só pelo `stdin` (`entregarSenha`). Sem askpass: o acesso é por chave/agente (BatchMode; V7).
import { hostKeyOptions } from '../ssh-command.ts'
import { HASH_LEITURA_ROOT, HASH_SONDAGEM, SCRIPT_LEITURA_ROOT, SCRIPT_SONDAGEM, sha256Texto } from './scripts.ts'

/** O destino que o dono informa. Só estes quatro campos entram no comando. */
export interface Alvo { host: string; port: number | null; user: string | null; identityFile: string | null }
export type MotivoAlvo = 'host' | 'usuario' | 'porta' | 'chave'

/** Mesmas regras de `validate()` em main/ssh.ts (host/alias, usuário, porta), mais o caminho absoluto da chave. */
export function validarAlvo(a: Alvo): MotivoAlvo | null {
  if (typeof a.host !== 'string' || a.host.length < 1 || a.host.length > 253 || a.host.startsWith('-') || !/^[A-Za-z0-9._-]+$/.test(a.host)) return 'host'
  if (a.user !== null && (typeof a.user !== 'string' || a.user.length > 64 || !/^[A-Za-z_][A-Za-z0-9._-]*$/.test(a.user))) return 'usuario'
  if (a.port !== null && !(Number.isInteger(a.port) && a.port >= 1 && a.port <= 65535)) return 'porta'
  if (a.identityFile !== null && !(typeof a.identityFile === 'string' && a.identityFile.startsWith('/') && a.identityFile.length <= 1024 && !/[\u0000-\u001f\u007f]/.test(a.identityFile))) return 'chave'
  return null
}

export class AlvoInvalido extends Error {
  readonly motivo: MotivoAlvo | 'pasta'
  constructor(motivo: MotivoAlvo | 'pasta') {
    super(`alvo-invalido:${motivo}`)
    this.motivo = motivo
  }
}

/** Pastas do app (home do Mac e pasta de dados): absolutas, sem aspas nem controle, porque entram entre aspas no `UserKnownHostsFile`. */
const pastaSegura = (p: string) => p.startsWith('/') && p.length <= 1024 && !/["\\\u0000-\u001f\u007f]/.test(p)

/** Comandos remotos: constantes. O texto do sondador vai por `stdin`; o do carregador root vai embutido, fixo. */
export const COMANDO_SONDAGEM = '/bin/sh -s'
const SUDO = '/usr/bin/sudo'
const SH = '/bin/sh'

/** Embrulha texto **fixo do app** (nunca do usuário) em aspas simples; recusa aspa simples dentro dele. */
function aspas(fixo: string): string {
  if (fixo.includes("'") || fixo.includes('\u0000')) throw new Error('texto-fixo-com-aspa-simples')
  return `'${fixo}'`
}

/** Comando remoto da sondagem (b) com senha: `sudo -S` lê a senha do `stdin`; `-k` ignora credencial em cache; prompt vazio. */
export const COMANDO_LEITURA_COM_SENHA = `${SUDO} -S -k -p '' ${SH} -c ${aspas(SCRIPT_LEITURA_ROOT)} mexerica-leitura`
/** V6: com `sudo -n` já aprovado pela sondagem (a), nenhuma senha é pedida. */
export const COMANDO_LEITURA_SEM_SENHA = `${SUDO} -n ${SH} -c ${aspas(SCRIPT_LEITURA_ROOT)} mexerica-leitura`

function base(a: Alvo, h: string, dir: string): string[] {
  const motivo = validarAlvo(a)
  if (motivo) throw new AlvoInvalido(motivo)
  if (!pastaSegura(h) || !pastaSegura(dir)) throw new AlvoInvalido('pasta')
  return [
    '-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-o', 'ForwardAgent=no', '-o', 'ForwardX11=no',
    '-o', 'ClearAllForwardings=yes', '-o', 'ControlMaster=no', '-o', 'ControlPath=none',
    // Sem proxy nem salto (A1): a impressão digital conferida é a do servidor do dono, não a de um intermediário.
    '-o', 'ProxyCommand=none', '-o', 'ProxyJump=none',
    // Nenhum comando local herdado do ssh_config do dono roda no Mac depois do login (`-o` da linha vale mais que o arquivo).
    '-o', 'PermitLocalCommand=no', '-o', 'LocalCommand=none',
    ...hostKeyOptions(h, dir),
    ...(a.port !== null ? ['-p', String(a.port)] : []),
    ...(a.identityFile !== null ? ['-i', a.identityFile, '-o', 'IdentitiesOnly=yes'] : []),
    ...(a.user !== null ? ['-l', a.user] : []),
  ]
}

/** Sondagem (a): script fixo por `stdin` (`SCRIPT_SONDAGEM`) de `ssh -T host /bin/sh -s`. */
export const argumentosSondagem = (a: Alvo, h: string, dir = `${h}/.mexerica`) => [...base(a, h, dir), '--', a.host, COMANDO_SONDAGEM]

/** Sondagem (b): `stdin` leva só a senha e `\n` (modo `com-senha`) ou nada (`sem-senha`, V6). */
export const argumentosLeituraRoot = (a: Alvo, h: string, modo: 'com-senha' | 'sem-senha', dir = `${h}/.mexerica`) =>
  [...base(a, h, dir), '--', a.host, modo === 'com-senha' ? COMANDO_LEITURA_COM_SENHA : COMANDO_LEITURA_SEM_SENHA]

/**
 * Ambiente do processo `ssh`: lista fechada. Nunca repassa o ambiente do app por inteiro (senha e tokens não passam
 * por aqui) e nunca inclui `SSH_ASKPASS*`.
 */
export function ambienteSsh(origem: Readonly<Record<string, string | undefined>>): Record<string, string> {
  const env: Record<string, string> = { LC_ALL: 'C' }
  for (const k of ['PATH', 'HOME', 'SSH_AUTH_SOCK'] as const) {
    const v = origem[k]
    if (typeof v === 'string' && v.length > 0 && !v.includes('\u0000')) env[k] = v
  }
  return env
}

/** Conferência dos textos fixos contra o hash esperado (F3 recusa rodar se não bater). */
export const textosFixosIntegros = () => sha256Texto(SCRIPT_SONDAGEM) === HASH_SONDAGEM && sha256Texto(SCRIPT_LEITURA_ROOT) === HASH_LEITURA_ROOT // hashes literais em scripts.ts

// MARK: senha (§4.4)

export const SENHA_MAX = 1024

/** Destino da cópia: `escrever` chama `pronto` quando o `stdin` terminou de aceitar os bytes (ou com o erro). */
export type EscreverSenha = (bytes: Buffer, pronto: (erro?: Error | null) => void) => void

/**
 * Entrega a senha ao `stdin` do `ssh` como `senha + \n` num Buffer novo e só zera os dois Buffers (o do chamador e a cópia)
 * **depois** que a escrita terminou (callback/backpressure) ou falhou: zerar antes corromperia o que ainda está na fila.
 * Recusa senha vazia, maior que 1024 bytes ou com `\n`/`\r`/NUL **antes** de alocar. Nunca devolve nem registra a senha.
 */
export async function entregarSenha(senha: Buffer, escrever: EscreverSenha): Promise<void> {
  let copia: Buffer | null = null
  try {
    if (senha.length < 1 || senha.length > SENHA_MAX || senha.some((b) => b === 0x0a || b === 0x0d || b === 0x00)) throw new RangeError('senha-fora-do-formato')
    copia = Buffer.alloc(senha.length + 1)
    senha.copy(copia)
    copia[senha.length] = 0x0a
    const bytes = copia
    await new Promise<void>((resolver, rejeitar) => {
      let feito = false
      escrever(bytes, (erro) => {
        if (feito) return
        feito = true
        if (erro) rejeitar(erro)
        else resolver()
      })
    })
  } finally {
    copia?.fill(0)
    senha.fill(0)
  }
}
