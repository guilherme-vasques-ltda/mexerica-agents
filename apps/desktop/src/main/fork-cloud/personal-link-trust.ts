/** D430 PR-A: âncoras SSH revisadas e embutidas, PRÓPRIAS do D430 (não a tabela PL1).
 * Vazia: nenhuma CA/destino real qualificado; o procedimento inicial fica FECHADO.
 * TOFU, accept-new, ssh-keyscan+sim ou conexão bem-sucedida nunca entram aqui. */
import type { SshTrust } from './personal-trust'
export interface PersonalLinkAnchor extends SshTrust {
  /** Comando remoto fixo do helper D430 na instalação isolada (sem shell do cliente). */
  readonly command: string
}
const ANCHORS: readonly Readonly<PersonalLinkAnchor>[] = Object.freeze([])
export function personalLinkAnchor(id: string): Readonly<PersonalLinkAnchor> | null { return ANCHORS.find(r => r.qualificationId === id) ?? null }
