import type { HostState } from './ipc.ts'
import type { SSHMotivo, SSHState, SSHStatus } from './ssh.ts'
import type { AppLanguage } from './fork-idioma.ts'
import { traduzir, traduzirPlural, type ChavePlural, type ChaveTexto } from '../renderer/lib/fork-i18n/index.ts'
import { isolado, ltr } from './bidi.ts'

// D454 (fatia 4): os textos vêm do catálogo (`ajustes.ssh.*`). Sem `idioma`, cada função mostra o que mostrava antes
// (inglês e, na linha da marca, pt-BR), para a barra do Mac e os chamadores ainda não migrados; com `idioma`, tudo
// segue a tela.
type Params = Record<string, string | number>
const texto = (idioma: AppLanguage, chave: string, params?: Params) => (traduzir as unknown as (i: AppLanguage, c: ChaveTexto, p?: Params) => string)(idioma, chave as ChaveTexto, params)
const plural = (idioma: AppLanguage, base: string, n: number) => (traduzirPlural as unknown as (i: AppLanguage, b: ChavePlural, n: number) => string)(idioma, base as ChavePlural, n)

// Comandos e caminhos ficam fora das frases do catálogo (iguais em todo idioma) e entram como parâmetro isolado.
export const COMANDO_INSTALAR_HOST = '~/.local/bin/donnabot-host install'
export const CAMINHO_HOST = '~/.local/bin/donnabot-host'
export const CAMINHO_HOST_DEV = '~/.local/bin/donnabot-dev-host'
export const COMANDO_IMPRESSAO_CHAVE = 'ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub'
export const comandoSSH = (host: string) => `ssh ${host}`

/** Parâmetros técnicos (host, porta, comando, caminho) vão da esquerda para a direita; o resto, isolado na direção dele. */
const TECNICOS = new Set(['host', 'porta', 'comando', 'caminho', 'alvo', 'email', 'codigo', 'exemplo'])
export function isolarParametros(params?: Params): Params | undefined {
  if (!params) return params
  return Object.fromEntries(Object.entries(params).map(([nome, valor]) => [nome, TECNICOS.has(nome) ? ltr(valor) : isolado(valor)]))
}

/**
 * Um motivo do SSH no idioma da tela, com host, comando e caminho isolados da frase (`shared/bidi.ts`): em árabe, o
 * bidi não troca a ordem deles. Sem idioma, o inglês base sem isolantes, como antes (barra do Mac).
 */
export function textoSSH(motivo: SSHMotivo, idioma?: AppLanguage): string {
  return idioma ? texto(idioma, motivo.chave, isolarParametros(motivo.params)) : texto('en', motivo.chave, motivo.params)
}

/** O motivo e o mesmo texto em inglês, sem isolantes, como o main grava no status (`message`/`step`). */
export function motivoSSH(chave: ChaveTexto, params?: Params): { motivo: SSHMotivo; texto: string } {
  const motivo: SSHMotivo = params ? { chave, params } : { chave }
  return { motivo, texto: textoSSH(motivo) }
}

export function isSSHConnecting(status: SSHStatus): boolean {
  return status.kind === 'connecting' || status.kind === 'retrying'
}

/** Saved remote computers keep their status and recovery controls reachable. */
export function showsChat(host: HostState['kind'], computers: number, ssh: SSHState): boolean {
  return host === 'running' || host === 'disabled' || computers > 0 || ssh.profiles.length > 0
}

/**
 * The menu bar's connection line when no bot needs anything: a connected computer, else what the
 * SSH computers are doing (D76: without a local host, the Hub's connection is the app's).
 */
export function connectionStatus(online: boolean, ssh: SSHState, idioma?: AppLanguage): string {
  const tr = (chave: ChaveTexto) => texto(idioma ?? 'en', chave)
  if (online) return tr('ajustes.ssh.estado.conectado')
  if (ssh.profiles.length === 0) return tr('ajustes.ssh.estado.nenhum')
  const statuses = ssh.profiles.map((p) => ssh.status[p.id] ?? { kind: 'idle' as const })
  if (statuses.some((s) => s.kind === 'confirmHostKey')) return traduzir(idioma ?? 'en', 'marca.barra.confirmeChave') // sem idioma, inglês base, como os outros status; a barra traduz
  if (statuses.some((s) => isSSHConnecting(s) || s.kind === 'connected')) return tr('ajustes.ssh.estado.conectando')
  if (statuses.some((s) => s.kind === 'failed' || s.kind === 'notInstalled')) return tr('ajustes.ssh.estado.falhou')
  return tr('ajustes.ssh.estado.desconectado')
}

/** Statuses here belong to profiles that have not attached a computer yet. */
export function sshSummary(statuses: SSHStatus[], idioma: AppLanguage = 'en'): string {
  const connecting = statuses.filter(isSSHConnecting).length
  const attention = statuses.filter((s) => s.kind === 'failed' || s.kind === 'notInstalled' || s.kind === 'confirmHostKey').length
  const opening = statuses.filter((s) => s.kind === 'connected').length
  const idle = statuses.filter((s) => s.kind === 'idle').length
  return [
    connecting && plural(idioma, 'ajustes.ssh.resumo.conectando', connecting),
    attention && plural(idioma, 'ajustes.ssh.resumo.atencao', attention),
    opening && plural(idioma, 'ajustes.ssh.resumo.abrindo', opening),
    idle && plural(idioma, 'ajustes.ssh.resumo.desconectado', idle),
  ].filter(Boolean).join(' · ')
}

/** O texto de um status com motivo no idioma pedido; sem motivo (status antigo), o texto que veio do main. */
const passo = (status: Extract<SSHStatus, { kind: 'connecting' }>, idioma: AppLanguage | undefined) => (status.motivo ? textoSSH(status.motivo, idioma) : status.step)
const falha = (status: Extract<SSHStatus, { kind: 'retrying' | 'failed' }>, idioma: AppLanguage | undefined) => (status.motivo ? textoSSH(status.motivo, idioma) : status.message)

/** Sem idioma, o inglês base sem isolantes (barra do Mac e chamadores antigos); com idioma, isolado para a tela. */
export function sshStatusDetail(status: SSHStatus, attached = true, idioma?: AppLanguage): string {
  const tr = (chave: ChaveTexto) => texto(idioma ?? 'en', chave)
  switch (status.kind) {
    case 'idle': return tr('ajustes.ssh.estado.desconectado')
    case 'connecting': return passo(status, idioma)
    case 'retrying': return texto(idioma ?? 'en', 'ajustes.ssh.tentandoDeNovo', { mensagem: idioma ? isolado(falha(status, idioma)) : falha(status, idioma) })
    case 'confirmHostKey': return tr('ajustes.ssh.estado.confirmeChave')
    case 'connected': return attached ? tr('ajustes.ssh.estado.conectado') : tr('ajustes.ssh.estado.abrindo')
    case 'failed': return falha(status, idioma)
    case 'notInstalled': return tr('ajustes.ssh.estado.semHost')
  }
}

/** O texto do status para a linha do perfil (SSHRow), no idioma da tela. */
export function textoDoStatus(status: Extract<SSHStatus, { kind: 'connecting' | 'retrying' | 'failed' }>, idioma: AppLanguage): string {
  return status.kind === 'connecting' ? passo(status, idioma) : falha(status, idioma)
}
