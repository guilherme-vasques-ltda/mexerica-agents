import { useEffect, useId, useRef, type ReactNode, type RefObject } from 'react'
import type { SignInProvider } from '@shared/analytics'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useAccount } from '../../../store/account'
import { ProviderMark } from '../../welcome/SignInMethods'
import { aparelhoDe, exitAnnouncement, exitCopy, exitLabel, exitWaiting, refusedNote } from './exit-copy'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import './account-exit.css'

/** As duas entradas de conta: Ajustes → Geral → Conta e a tela de boas-vindas (sempre escura). */
export type AccountSurface = 'settings' | 'welcome'

/** D185: falha transitória oferece nova tentativa ou limpeza para entrar novamente. */
export function AccountExitPanel({ surface }: { surface: AccountSurface }) {
  const account = useAccount()
  const aparelho = aparelhoDe(window.codync.platform)
  const idioma = useIdioma()
  const titleId = useId()
  const panel = useRef<HTMLDivElement>(null)
  const holdFocus = useFocusReturn(panel, account.canAct)
  const kind = account.exit
  const copy = exitCopy(kind, { error: account.errorMessage, aparelho, idioma, hasSession: account.isSignedIn, outcome: account.exitOutcome })
  const waiting = exitWaiting(account.exitAction, account.isSigningOut, idioma)
  const refused = !waiting && account.exitOutcome?.kind === 'unavailable'
  const start = (run: () => Promise<void>) => { holdFocus(); void run() }
  const buttons: ReactNode[] = [
    <ExitButton key="check" surface={surface} primary busy={account.exitAction === 'check'} disabled={!account.canAct} onClick={() => start(() => account.checkExit())}>{exitLabel('check', { kind, busy: account.exitAction === 'check', idioma })}</ExitButton>,
    <ExitButton key="signOut" surface={surface} primary={false} busy={account.isSigningOut} disabled={account.isSigningOut} onClick={() => start(() => account.requestSignOut())}>{exitLabel('signOut', { kind, busy: account.isSigningOut, idioma })}</ExitButton>,
  ]
  const mark = (
    <span className="account-exit-mark" aria-hidden>
      <Icon name="exclamationmark.triangle" size={surface === 'welcome' ? 12 : 17} scaled={surface !== 'welcome'} />
    </span>
  )
  return (
    <div ref={panel} className={`account-exit is-${surface} idioma-superficie`} {...superficieIdioma(idioma)} role="group" aria-labelledby={titleId}>
      {surface === 'settings' ? mark : null}
      <div className="account-exit-text">
        <p id={titleId} className="account-exit-title">
          {surface === 'welcome' ? mark : null}
          {copy.title}
        </p>
        <p className="account-exit-body">{copy.body}</p>
        {copy.hint ? <p className="account-exit-hint">{copy.hint}</p> : null}
        {copy.extra ? <p className="account-exit-hint">{copy.extra}</p> : null}
      </div>
      {buttons.length > 0 ? <div className="account-exit-actions">{buttons}</div> : null}
      {waiting || refused ? (
        <p className={`account-exit-waiting${refused ? ' is-refused' : ''}`}>
          {waiting ? <BusyMark size={12} /> : null}
          {waiting || refusedNote(idioma)}
        </p>
      ) : null}

    </div>
  )
}


export function AccountExitLive({ fallback = '' }: { fallback?: string }) {
  const account = useAccount()
  const idioma = useIdioma()
  const text = exitAnnouncement({ error: account.errorMessage, kind: account.exit, busy: account.exitAction, signingOut: account.isSigningOut, outcome: account.exitOutcome, hasSession: account.isSignedIn, aparelho: aparelhoDe(window.codync.platform), idioma })
  return <p className="account-exit-live idioma-superficie" {...superficieIdioma(idioma)} role="status" aria-live="polite">{text || fallback}</p>
}

function ExitButton({ surface, primary, busy, disabled, provider, onClick, children }: {
  surface: AccountSurface
  primary: boolean
  busy: boolean
  disabled: boolean
  provider?: SignInProvider
  onClick: () => void
  children: string
}) {
  if (surface === 'settings') {
    return (
      <Button kind={primary ? 'primary' : 'secondary'} disabled={disabled} onClick={onClick}>
        {busy ? <BusyMark size={13} /> : null}
        {children}
      </Button>
    )
  }
  return (
    <button className="welcome-ghost" type="button" disabled={disabled} onClick={onClick}>
      {busy ? <BusyMark size={13} /> : provider ? <ProviderMark provider={provider} size={13} /> : null}
      {children}
    </button>
  )
}

/** O texto ao lado já diz o que está em andamento; o rótulo do Spinner compartilhado fica de fora. */
function BusyMark({ size }: { size: number }) {
  return (
    <span className="account-exit-spin" aria-hidden>
      <Spinner size={size} />
    </span>
  )
}

/**
 * O botão que se desabilita enquanto sua ação roda perde o foco do teclado: ao terminar, o foco
 * volta para a primeira ação habilitada do painel, se não foi para outro lugar.
 */
export function useFocusReturn(panel: RefObject<HTMLElement | null>, idle: boolean, selector = '.account-exit-actions button:not(:disabled)') {
  const held = useRef(false)
  useEffect(() => {
    if (!idle || !held.current) return
    held.current = false
    const node = panel.current
    const active = document.activeElement
    if (!node || (active && active !== document.body && active !== document.documentElement)) return
    node.querySelector<HTMLButtonElement>(selector)?.focus({ preventScroll: true })
  }, [idle, panel, selector])
  return () => {
    held.current = true
  }
}
