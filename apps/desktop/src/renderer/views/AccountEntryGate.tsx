import { useEffect, useState, type ReactNode } from 'react'
import { showsAccountControls } from '@shared/fork-features'
import { idiomaDoDocumento, superficieIdioma, tradutor } from '../lib/fork-i18n/index.ts'
import { Dialog } from '../components/Overlay'
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
  return (
    <>
      {entry === 'confirmed' ? children : <LockedWindow entry={entry} />}
      <ConfirmarSairWeb />
    </>
  )
}

/** A janela antes da conta: as boas-vindas e o "Reset all data…" da bandeja. Nenhum outro comando chega aqui. */
function LockedWindow({ entry }: { entry: Exclude<AccountEntry, 'confirmed'> }) {
  const [confirmReset, setConfirmReset] = useState(false)
  useEffect(() => window.codync.app.onCommand((c) => { if (c.kind === 'confirmReset') setConfirmReset(true) }), [])
  return (
    <div className="chat-window">
      <AccountWelcomeView entry={entry} />
      <div className="window-drag drag" />
      <ResetDataDialog open={confirmReset} onClose={() => setConfirmReset(false)} />
    </div>
  )
}

/** "Apagar todos os dados" (bandeja → Reset all data…): o mesmo no portão e no app. */
export function ResetDataDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  // A porta não lê preferências (account-entry.test): o idioma é o que o <html> mostra.
  const idioma = idiomaDoDocumento()
  const t = tradutor(idioma)
  return (
    <Dialog
      open={open}
      title={t('navegacao.conta.apagarDados')}
      message={t('marca.conta.apagarTudo')}
      cancel={t('navegacao.janela.cancelar')}
      language={superficieIdioma(idioma)}
      actions={[{ title: t('navegacao.conta.apagarTudo'), destructive: true, action: () => void window.codync.app.resetAllData() }]}
      onClose={onClose}
    />
  )
}
