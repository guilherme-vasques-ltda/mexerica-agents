import { useEffect, useRef, useState } from 'react'
import { useIdioma, useT, type Tradutor } from '../../lib/fork-i18n/estado'
import type { SignInProvider } from '@shared/analytics'
import type { SecondFactorStrategy } from '@shared/ipc'
import { useAccount } from '../../store/account'
import type { AccountEntry } from '../AccountEntryGate'
import { AccountExitLive, AccountExitPanel } from '../fork/account/AccountExitPanel'
import { revokedCopy } from '../fork/account/exit-copy'
import { BusyMark, CodeForm, newAccountOffer, PasswordForm, ProviderMark, providerName, visibleProviders } from './SignInMethods'
import { traduzir } from '../../lib/fork-i18n/index.ts'

/**
 * Login obrigatório (dono, 31/10/2026): entrar é o único caminho para o app, sem "Conhecer a Donna".
 * E6a (05/11/2026): Criar conta ao lado de Entrar e as telas de código (segundo fator, e-mail do cadastro).
 * E7c (05/11/2026): com o cadastro do Clerk em lista de espera, "Entrar na lista de espera" no lugar de Criar conta;
 * sem leitura do modo, nenhum dos dois. O link abre a página fixa do Main; a recusa do Google mostra o mesmo link.
 * D177 (01/11/2026): e-mail e senha na ação principal; Apple, X e Google abaixo, no mesmo peso, só os que
 * a instância Clerk tiver ligados (lista do Main; sem ela, só e-mail e senha). Com a
 * falha transitória, retry ou entrada nova aparecem no lugar do login. Com o login ligado mas não
 * configurado, só o aviso de indisponível (a versão sem login abre direto, dono 05/11/2026).
 */
export function WelcomeActions({ entry }: { entry: Exclude<AccountEntry, 'confirmed' | 'restoring'> }) {
  return (
    <div className="welcome-cta">
      {entry === 'unavailable' ? (
        <p className="welcome-cloud-note">{traduzir('pt-BR', 'marca.conta.semLogin')}</p>
      ) : (
        <AccountSignIn recovery={entry === 'recovery'} />
      )}
    </div>
  )
}

/** O método que esta tela começou: só escolhe onde aparece a espera. A sessão continua vindo do Main. */
type Started = SignInProvider | 'password' | 'signup' | 'code' | 'email_code' | 'forgot'

function waitingCopy(t: Tradutor, started: Started) {
  if (started === 'password') return t('navegacao.conta.espera.senha')
  if (started === 'signup') return t('navegacao.conta.espera.criando')
  if (started === 'code') return t('navegacao.conta.espera.codigo')
  if (started === 'email_code' || started === 'forgot') return t('navegacao.conta.espera.enviando')
  return t('navegacao.conta.espera.navegador', { provedor: providerName[started] })
}

function AccountSignIn({ recovery }: { recovery: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  const campos = { email: t('navegacao.conta.email'), senha: t('navegacao.conta.senha') }
  const account = useAccount()
  const [started, setStarted] = useState<Started | null>(null)
  const [mode, setMode] = useState<'signin' | 'signup'>('signin')
  const disabled = !account.isConfigured || !account.canAct
  const waiting = account.isBusy ? started : null
  // D185: somente falha transitória/limpeza local pendente mantém a recuperação.
  const revoked = account.exit === 'revoked' && !account.isSignedIn
  const providers = visibleProviders(account.socialProviders)
  const offer = newAccountOffer(account.signUpMode, account.signUp, account.waitlist)
  const signUp = offer === 'signup' ? account.signUp : null
  const creating = mode === 'signup' && !!signUp
  const email = useRef<HTMLInputElement>(null)
  const [needEmail, setNeedEmail] = useState(false)
  const emailCode = account.emailCode && !creating
  /** Código por e-mail e Esqueci minha senha usam o e-mail da cápsula, lido só no clique; sem ele, foco no campo. */
  const withEmail = (kind: Started, run: (typed: string) => Promise<void>) => {
    const typed = email.current?.value.trim() ?? ''
    if (!typed) { setNeedEmail(true); email.current?.focus(); return }
    setNeedEmail(false)
    setStarted(kind)
    void run(typed)
  }
  const notes = (
    <>
      {waiting ? <p className="welcome-cloud-note" role="status">{waitingCopy(t, waiting)}</p> : null}
      {revoked ? <p className="welcome-cloud-note">{`${revokedCopy(idioma).title}. ${revokedCopy(idioma).body}`}</p> : null}
      {account.errorMessage ? <p className="welcome-cloud-note is-error" role="alert">{account.errorMessage}</p> : null}
    </>
  )
  if (!recovery && account.pending) {
    return (
      <div className="welcome-cloud">
        {account.pending.kind === 'second_factor' ? (
          <SecondFactorStep strategies={account.pending.strategies} busy={waiting === 'code'} disabled={disabled} onStart={() => setStarted('code')} />
        ) : account.pending.kind === 'new_password' ? (
          <NewPasswordStep minPasswordLength={account.pending.minPasswordLength} busy={waiting === 'code'} disabled={disabled} onStart={() => setStarted('code')} />
        ) : (
          <EmailCodeStep purpose={account.pending.purpose} resendAt={account.pending.resendAt} busy={waiting === 'code'} disabled={disabled} onStart={() => setStarted('code')} />
        )}
        {notes}
        <AccountExitLive />
      </div>
    )
  }
  return (
    <div className="welcome-cloud">
      {recovery ? (
        <AccountExitPanel surface="welcome" />
      ) : (
        <>
          {signUp ? (
            <div className="welcome-mode" role="group" aria-label={t('navegacao.conta.modo')}>
              <button type="button" className="welcome-mode-option" aria-pressed={!creating} disabled={disabled} onClick={() => setMode('signin')}>{t('navegacao.conta.entrar')}</button>
              <button type="button" className="welcome-mode-option" aria-pressed={creating} disabled={disabled} onClick={() => setMode('signup')}>{t('navegacao.conta.criar')}</button>
            </div>
          ) : null}
          {creating ? (
            <>
              <PasswordForm
                campos={campos}
                key="signup"
                label={t('navegacao.conta.criarComSenha')}
                action={t('navegacao.conta.criar')}
                tone="primary"
                newPassword
                busy={waiting === 'signup'}
                disabled={disabled}
                onSubmit={({ identifier, password }) => {
                  setStarted('signup')
                  void account.signUpWithPassword(identifier, password)
                }}
              />
              <p className="welcome-cloud-note">{t('navegacao.conta.senhaMinima', { n: signUp.minPasswordLength })}</p>
            </>
          ) : (
            <PasswordForm
              campos={campos}
              key="signin"
              emailRef={email}
              label={t('navegacao.conta.entrarComSenha')}
              action={t('navegacao.conta.entrar')}
              tone="primary"
              busy={waiting === 'password'}
              disabled={disabled}
              onSubmit={({ identifier, password }) => {
                setStarted('password')
                void account.signInWithPassword(identifier, password)
              }}
            />
          )}
          {!creating && account.passwordReset ? (
            <button type="button" className="welcome-link welcome-forgot" disabled={disabled} onClick={() => withEmail('forgot', (typed) => account.forgotPassword(typed))}>
              {waiting === 'forgot' ? <BusyMark size={11} /> : null}
              {t('navegacao.conta.esqueci')}
            </button>
          ) : null}
          {providers.length || emailCode ? (
            <div className="welcome-providers" role="group" aria-label={t(creating ? 'navegacao.conta.criarComOutra' : 'navegacao.conta.entrarComOutra')}>
              <span aria-hidden>{t(creating ? 'navegacao.conta.ouCrie' : 'navegacao.conta.ouEntre')}</span>
              {emailCode ? (
                <button
                  className="welcome-ghost"
                  type="button"
                  aria-label={t('navegacao.conta.entrarComCodigo')}
                  disabled={disabled}
                  onClick={() => withEmail('email_code', (typed) => account.signInWithEmailCode(typed))}
                >
                  {waiting === 'email_code' ? <BusyMark size={13} /> : <span className="welcome-x-mark" aria-hidden style={{ fontSize: 14 }}>✉</span>}
                  {t('navegacao.conta.codigoPorEmail')}
                </button>
              ) : null}
              {providers.map((provider) => (
                <button
                  key={provider}
                  className="welcome-ghost"
                  type="button"
                  aria-label={t('navegacao.conta.entrarCom', { provedor: providerName[provider] })}
                  disabled={disabled}
                  onClick={() => {
                    setStarted(provider)
                    void account.signIn(provider)
                  }}
                >
                  {waiting === provider ? <BusyMark size={13} /> : <ProviderMark provider={provider} size={13} />}
                  {providerName[provider]}
                </button>
              ))}
            </div>
          ) : null}
          {needEmail && !creating ? <p className="welcome-cloud-note">{t('navegacao.conta.digiteEmail')}</p> : null}
          {notes}
          {offer === 'waitlist' ? (
            <p className="welcome-cloud-note welcome-waitlist">
              {account.waitlist ? null : <span>{t('navegacao.conta.semConvite')} </span>}
              <button type="button" className="welcome-link" disabled={account.isBusy} onClick={() => void account.openWaitlist()}>{t('navegacao.conta.listaEspera')}</button>
            </p>
          ) : null}
        </>
      )}
      <AccountExitLive />
    </div>
  )
}

/** E6a: segundo fator. Autenticador primeiro; o código de recuperação fica atrás de um link. */
function SecondFactorStep({ strategies, busy, disabled, onStart }: { strategies: SecondFactorStrategy[]; busy: boolean; disabled: boolean; onStart: () => void }) {
  const t = useT()
  const account = useAccount()
  const [strategy, setStrategy] = useState<SecondFactorStrategy>(strategies[0] ?? 'totp')
  const other = strategies.find((s) => s !== strategy)
  const totp = strategy === 'totp'
  return (
    <div className="welcome-step" role="group" aria-labelledby="welcome-step-title">
      <p id="welcome-step-title" className="welcome-step-title">{t('navegacao.conta.doisFatores')}</p>
      <p className="welcome-cloud-note">
        {t(totp ? 'navegacao.conta.digiteAutenticador' : 'navegacao.conta.digiteRecuperacao')}
      </p>
      <CodeForm
        key={strategy}
        label={t(totp ? 'navegacao.conta.codigoAutenticador' : 'navegacao.conta.codigoRecuperacao')}
        placeholder={totp ? '000000' : t('navegacao.conta.codigo')}
        numeric={totp}
        action={t('navegacao.conta.confirmar')}
        busy={busy}
        disabled={disabled}
        onSubmit={(code) => {
          onStart()
          void account.attemptSecondFactor(strategy, code)
        }}
      />
      <div className="welcome-step-links">
        {other ? (
          <button type="button" className="welcome-link" disabled={disabled} onClick={() => setStrategy(other)}>
            {t(other === 'backup_code' ? 'navegacao.conta.usarRecuperacao' : 'navegacao.conta.usarAutenticador')}
          </button>
        ) : null}
        <button type="button" className="welcome-link" disabled={account.isBusy} onClick={() => void account.cancelPending()}>{t('navegacao.janela.cancelar')}</button>
      </div>
    </div>
  )
}

/** E6a: código do e-mail (cadastro ou entrar sem senha), com reenviar depois da espera que o Main impõe. */
function EmailCodeStep({ purpose, resendAt, busy, disabled, onStart }: { purpose: 'sign_up' | 'sign_in' | 'device_trust' | 'reset_password'; resendAt: number; busy: boolean; disabled: boolean; onStart: () => void }) {
  const t = useT()
  const account = useAccount()
  const [now, setNow] = useState(() => Date.now())
  const wait = Math.max(0, Math.ceil((resendAt - now) / 1000))
  // Só atualiza o rótulo da espera; nada é enviado sozinho.
  useEffect(() => {
    if (wait <= 0) return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [wait])
  return (
    <div className="welcome-step" role="group" aria-labelledby="welcome-step-title">
      <p id="welcome-step-title" className="welcome-step-title">{t(({ sign_up: 'navegacao.conta.titulo.confirmeEmail', sign_in: 'navegacao.conta.titulo.entrarCodigo', device_trust: 'navegacao.conta.titulo.confirmeVoce', reset_password: 'navegacao.conta.titulo.redefinir' } as const)[purpose])}</p>
      <p className="welcome-cloud-note">
        {t(purpose === 'device_trust' ? 'navegacao.conta.primeiraEntrada' : 'navegacao.conta.enviamos')}
      </p>
      <CodeForm
        label={t('navegacao.conta.codigoEmail')}
        placeholder="000000"
        numeric
        action={t('navegacao.conta.confirmar')}
        busy={busy}
        disabled={disabled}
        onSubmit={(code) => {
          onStart()
          void account.verifyEmailCode(code)
        }}
      />
      <div className="welcome-step-links">
        <button type="button" className="welcome-link" disabled={disabled || wait > 0} onClick={() => void account.resendEmailCode()}>
          {wait > 0 ? t('navegacao.conta.reenviarEm', { s: wait }) : t('navegacao.conta.reenviar')}
        </button>
        <button type="button" className="welcome-link" disabled={account.isBusy} onClick={() => void account.cancelPending()}>{t('navegacao.janela.cancelar')}</button>
      </div>
    </div>
  )
}

/** E6a: nova senha depois do código de "Esqueci minha senha". A senha fica só no campo, como no login. */
function NewPasswordStep({ minPasswordLength, busy, disabled, onStart }: { minPasswordLength: number; busy: boolean; disabled: boolean; onStart: () => void }) {
  const t = useT()
  const account = useAccount()
  return (
    <div className="welcome-step" role="group" aria-labelledby="welcome-step-title">
      <p id="welcome-step-title" className="welcome-step-title">{t('navegacao.conta.novaSenhaTitulo')}</p>
      <p className="welcome-cloud-note">{t('navegacao.conta.novaSenhaDica', { n: minPasswordLength })}</p>
      <CodeForm
        label={t('navegacao.conta.novaSenha')}
        placeholder={t('navegacao.conta.novaSenha')}
        numeric={false}
        secret
        action={t('navegacao.conta.salvarEntrar')}
        busy={busy}
        disabled={disabled}
        onSubmit={(password) => {
          onStart()
          void account.resetPassword(password)
        }}
      />
      <div className="welcome-step-links">
        <button type="button" className="welcome-link" disabled={account.isBusy} onClick={() => void account.cancelPending()}>{t('navegacao.janela.cancelar')}</button>
      </div>
    </div>
  )
}
