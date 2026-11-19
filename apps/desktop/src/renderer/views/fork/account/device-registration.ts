// D171 (Clerk 7): o registro normal deste aparelho na conta, depois do login (CloudModel). Só uma
// tentativa sai sozinha no login; depois de uma falha, tentar de novo é um clique, na mesma sessão,
// sem temporizador nem outro login. Registrar não aprova o aparelho em nenhum computador.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Aparelho } from './exit-copy'

export type DeviceRegistration = 'idle' | 'registering' | 'registered' | 'failed'

/** A parte do CloudModel que a tela usa: ele mesmo descarta respostas de outra conta ou sessão. */
export interface RegistrationModel {
  readonly deviceRegistration: DeviceRegistration
  registerThisDevice(quiet?: boolean): Promise<boolean>
  refresh(quiet?: boolean): Promise<boolean>
}

/** A parte da conta que cerca a tentativa: nada em andamento e a mesma sessão ao voltar. */
export interface RegistrationAccount {
  readonly canAct: boolean
  readonly isSignedIn: boolean
  readonly sessionRevision: number
}

/** A retomada aparece depois de uma falha desta sessão e, enquanto roda, só se veio de um clique. */
export function offersRegistrationRetry(state: DeviceRegistration, retried: boolean) {
  return state === 'failed' || (state === 'registering' && retried)
}

/**
 * Um clique em "Registrar de novo". Silencioso no CloudModel (a falha aparece no cartão, não num
 * diálogo de erro); lista os computadores da conta de novo só se registrou e a sessão é a mesma.
 */
export async function retryDeviceRegistration(cloud: RegistrationModel, account: RegistrationAccount): Promise<boolean> {
  if (!account.canAct || !account.isSignedIn || cloud.deviceRegistration === 'registering') return false
  const revision = account.sessionRevision
  const registered = await cloud.registerThisDevice(true)
  if (registered && account.sessionRevision === revision && account.isSignedIn) void cloud.refresh(true)
  return registered
}

export interface RegistrationCopy {
  /** A nota sob o e-mail; nula quando nada precisa ser dito. */
  detail: string | null
  /** O rodapé do cartão. */
  footer: string | null
  /** O que um leitor de tela ouve. */
  announcement: string
}

export function registrationCopy(state: DeviceRegistration, aparelho: Aparelho, retried: boolean, idioma: AppLanguage): RegistrationCopy {
  switch (state) {
    case 'registering': {
      const texto = traduzir(idioma, `conta.registro.registrando.${aparelho}`)
      return { detail: texto, footer: null, announcement: texto }
    }
    case 'failed':
      return {
        detail: traduzir(idioma, `conta.registro.falhou.${aparelho}`),
        footer: traduzir(idioma, `conta.registro.incerto.${aparelho}`),
        announcement: traduzir(idioma, `conta.registro.naoConfirmado.${aparelho}`),
      }
    case 'registered':
      return { detail: null, footer: null, announcement: retried ? traduzir(idioma, `conta.registro.registrado.${aparelho}`) : '' }
    case 'idle':
      return { detail: null, footer: null, announcement: '' }
  }
}

export function registrationLabel(state: DeviceRegistration, idioma: AppLanguage) {
  return traduzir(idioma, state === 'registering' ? 'conta.registro.registrandoBotao' : 'conta.registro.deNovo')
}
