import type { ExitAction, ExitIntent, ExitKind } from '../../../store/account'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

/**
 * DON-917 (fatia 9b): o tipo do aparelho escolhe a frase inteira do catálogo (`….mac` ou `….pc`), sem montar
 * "este Mac"/"deste Mac" em pedaços. O idioma vem de quem desenha.
 */
export type Aparelho = 'mac' | 'pc'
export const aparelhoDe = (platform: string): Aparelho => (platform === 'darwin' ? 'mac' : 'pc')

type Outcome = { action: ExitIntent; kind: ExitKind; uncertain?: boolean } | null
// E6a (DON-863): sem internet, "Tentar de novo" é o botão certo; "Entrar novamente" apaga o login local.
export function exitCopy(_kind: ExitKind, { aparelho, idioma, error }: { error?: string | null; aparelho: Aparelho; idioma: AppLanguage; hasSession: boolean; outcome: Outcome }) {
  const reason = error === 'keychain.unavailable' ? 'unavailable' : error === 'keychain.mismatch' ? 'mismatch' : error === 'keychain.unsupported' ? 'unsupported' : null
  if (reason) return { title: traduzir(idioma, 'sistema.barra.cofre.titulo'), body: traduzir(idioma, `sistema.barra.cofre.${reason}`), hint: '', extra: null }
  return {
    title: traduzir(idioma, 'conta.saida.titulo'),
    body: traduzir(idioma, `conta.saida.corpo.${aparelho}`),
    hint: traduzir(idioma, `conta.saida.dica.${aparelho}`),
    extra: null,
  }
}
export function exitLabel(action: ExitAction, { busy, idioma }: { kind: ExitKind; busy: boolean; idioma: AppLanguage }): string {
  return traduzir(idioma, action === 'check' ? busy ? 'conta.saida.tentando' : 'conta.saida.tentar' : busy ? 'conta.saida.saindo' : 'conta.saida.entrarNovamente')
}
export function exitWaiting(busy: ExitIntent | null, signingOut: boolean, idioma: AppLanguage): string {
  return signingOut ? traduzir(idioma, 'conta.saida.limpando') : busy ? traduzir(idioma, 'conta.saida.verificando') : ''
}
export const refusedNote = (idioma: AppLanguage) => traduzir(idioma, 'conta.saida.recusado')
export function exitAnnouncement({ kind, busy, signingOut, outcome, hasSession, aparelho, idioma, error }: { error?: string | null; kind: ExitKind; busy: ExitIntent | null; signingOut: boolean; outcome: Outcome; hasSession: boolean; aparelho: Aparelho; idioma: AppLanguage }): string {
  return exitWaiting(busy, signingOut, idioma) || (outcome?.kind === 'unavailable' ? refusedNote(idioma) : kind === 'none' ? '' : exitCopy(kind, { hasSession, aparelho, idioma, outcome, error }).title)
}

// Compatibility wording for a previously delivered projection; no remote proof is implied.
export const revokedCopy = (idioma: AppLanguage) => ({ title: traduzir(idioma, 'conta.revogada.titulo'), body: traduzir(idioma, 'conta.revogada.corpo'), hint: traduzir(idioma, 'conta.revogada.dica') })
