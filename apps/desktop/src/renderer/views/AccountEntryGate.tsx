import { useEffect, type ReactNode } from 'react'
import { showsAccountControls } from '@shared/fork-features'
import { useAccount } from '../store/account'
import { AccountWelcomeView } from './AccountWelcomeView'
import { ConfirmarSairWeb } from './fork/account/ConfirmarSairWeb'

/** O que a conta deixa esta janela mostrar. Só `confirmed` monta o app. */
export type AccountEntry = 'restoring' | 'unavailable' | 'recovery' | 'signed_out' | 'confirmed'

/** O que o portão lê: a projeção do processo principal (e a saída pedida aqui), nunca uma preferência. */
export interface AccountEntryState {
  readonly isReady: boolean
  readonly isConfigured: boolean
  readonly isSignedIn: boolean
  readonly isBusy: boolean
  readonly isSigningOut: boolean
  readonly isExitUnresolved: boolean
}

/**
 * Login obrigatório (dono, 31/10/2026): numa versão com login, o app abre só com a conta que o processo
 * principal confirmou; com o login ligado mas não configurado, fica indisponível, sem atalho offline.
 * Exceção (dono, 05/11/2026, opção B): a versão gerada sem sistema de login (`cloudAccess` desligado,
 * sem chaves do Clerk) abre direto, porque nela não há conta com que entrar. Restaurando, com saída não resolvida ou sessão por verificar, entrando ou saindo, a janela
 * fica no portão. `onboardingDone` não conta: não é lido aqui.
 */
export function accountEntry(account: AccountEntryState, controls: boolean): AccountEntry {
  if (!controls) return 'confirmed'
  if (!account.isReady) return 'restoring'
  if (account.isExitUnresolved) return 'recovery'
  if (!account.isConfigured) return 'unavailable'
  if (account.isSignedIn && !account.isBusy && !account.isSigningOut) return 'confirmed'
  return 'signed_out'
}

/**
 * Monta `children` (o app inteiro, com seus anexos, comandos e resumos) só com a conta confirmada.
 * Ao sair, perder ou deixar de verificar a sessão, o app é desmontado e o portão volta.
 */
export function AccountEntryGate({ children }: { children: ReactNode }) {
  const account = useAccount()
  const entry = accountEntry(account, showsAccountControls())
  useEffect(() => window.codync.app.onCommand((comando) => {
    if (comando.kind === 'confirmReset') void window.codync.app.resetAllData()
  }), [])
  return (
    <>
      {entry === 'confirmed' ? children : (
        <div className="chat-window">
          <AccountWelcomeView entry={entry} />
          <div className="window-drag drag" />
        </div>
      )}
      <ConfirmarSairWeb />
    </>
  )
}
