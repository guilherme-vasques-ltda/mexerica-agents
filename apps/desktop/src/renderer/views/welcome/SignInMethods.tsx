import { useEffect, useRef, useState, type RefObject } from 'react'
import type { SignInProvider } from '@shared/analytics'
import { Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'

// O logotipo do X é um símbolo (𝕏), igual em todo idioma.
const MARCA_X = '\u{1D54F}' // textos-ok: logotipo do X, não é texto de interface
import googleLogo from '../../assets/google.svg'

export { newAccountOffer, visibleProviders } from '../../../shared/fork-account-providers'

export const providerName: Record<SignInProvider, string> = { apple: 'Apple', x: 'X', google: 'Google' }

/**
 * A marca de cada provedor, decorativa: o nome legível vem no botão. X usa o caractere 𝕏 da fonte do
 * sistema, sem arquivo de logo copiado; a marca oficial exige origem e licença registradas.
 */
export function ProviderMark({ provider, size }: { provider: SignInProvider; size: number }) {
  switch (provider) {
    case 'apple':
      return <Icon name="provedor.apple" size={size} scaled={false} />
    case 'google':
      return <img src={googleLogo} alt="" width={size} height={size} />
    case 'x':
      return <span className="welcome-x-mark" aria-hidden style={{ fontSize: size + 2 }}>{MARCA_X}</span>
  }
}

/** Conta ocupada: o botão desabilitado já diz; o rótulo do Spinner compartilhado ("Loading") fica de fora. */
export function BusyMark({ size }: { size: number }) {
  return (
    <span className="welcome-spin" aria-hidden>
      <Spinner size={size} />
    </span>
  )
}

export interface Credentials {
  identifier: string
  password: string
}

/**
 * Lê e-mail e senha dos campos. Com os dois preenchidos, apaga a senha do campo na hora e devolve os
 * dois; faltando algum, não muda nada. O e-mail continua no campo para uma nova tentativa.
 */
export function takeCredentials(identifier: { value: string }, password: { value: string }): Credentials | null {
  const typed = identifier.value.trim()
  const secret = password.value
  if (!typed || !secret) return null
  password.value = ''
  return { identifier: typed, password: secret }
}

/**
 * E-mail e senha numa cápsula (D177). A senha fica só no campo: nunca no estado do React, em
 * preferência, disco, log ou análise de uso. Ela sai do campo ao enviar, quando qualquer ação de conta
 * começa (`disabled`) e ao desmontar. Envia só por clique ou Enter no próprio formulário; nada é
 * preenchido, focado ou enviado sozinho. A sessão continua sendo do processo principal.
 */
export function PasswordForm({ label, action, tone, busy, disabled, onSubmit, newPassword = false, emailRef, campos }: {
  /** Fatia 1 (D454): os rótulos dos campos vêm de quem chama, já traduzidos (este módulo não lê preferência). */
  campos: { email: string; senha: string }
  /** E6a: o e-mail digitado também serve a "Código por e-mail" (lido só no clique). */
  emailRef?: RefObject<HTMLInputElement | null>
  /** Nome acessível do formulário. */
  label: string
  /** Criar conta: o gerenciador de senhas sugere uma senha nova (`new-password`). */
  newPassword?: boolean
  /** Texto do botão de envio. */
  action: string
  /** `primary` no login; `ghost` na recuperação, no mesmo peso das outras confirmações. */
  tone: 'primary' | 'ghost'
  busy: boolean
  disabled: boolean
  onSubmit: (credentials: Credentials) => void
}) {
  const ownIdentifier = useRef<HTMLInputElement>(null)
  const identifier = emailRef ?? ownIdentifier
  const password = useRef<HTMLInputElement>(null)
  const [filled, setFilled] = useState(false)
  const update = () => setFilled(!!identifier.current?.value.trim() && !!password.current?.value)

  // Qualquer ação de conta em curso, inclusive Apple, X ou Google, leva a senha digitada.
  useEffect(() => {
    if (!disabled) return
    if (password.current) password.current.value = ''
    setFilled(false)
  }, [disabled])
  // Ao desmontar (conta confirmada, recuperação, outro estado), o campo sai vazio.
  useEffect(() => {
    const field = password.current
    return () => {
      if (field) field.value = ''
    }
  }, [])

  return (
    <form
      className="welcome-password"
      aria-label={label}
      noValidate
      onSubmit={(event) => {
        event.preventDefault()
        if (disabled || !identifier.current || !password.current) return
        const credentials = takeCredentials(identifier.current, password.current)
        setFilled(false)
        if (credentials) onSubmit(credentials)
      }}
    >
      <input
        ref={identifier}
        className="welcome-field"
        type="email"
        dir="auto"
        aria-label={campos.email}
        placeholder={campos.email}
        autoComplete="username"
        autoCapitalize="off"
        spellCheck={false}
        disabled={disabled}
        onChange={update}
      />
      <span className="welcome-field-divider" aria-hidden />
      <input
        ref={password}
        className="welcome-field"
        type="password"
        dir="auto"
        aria-label={campos.senha}
        placeholder={campos.senha}
        autoComplete={newPassword ? 'new-password' : 'current-password'}
        disabled={disabled}
        onChange={update}
      />
      <button className={tone === 'primary' ? 'welcome-primary' : 'welcome-ghost'} type="submit" disabled={disabled || !filled}>
        {busy ? <BusyMark size={tone === 'primary' ? 15 : 13} /> : null}
        {action}
      </button>
    </form>
  )
}

/**
 * E6a: um código (autenticador, recuperação ou e-mail) na mesma cápsula. Como a senha, fica só no campo:
 * sai dele ao enviar, quando uma ação começa e ao desmontar. Envia só por clique ou Enter, nunca ao
 * completar os dígitos.
 */
export function CodeForm({ label, placeholder, numeric, secret = false, action, busy, disabled, onSubmit }: {
  label: string
  /** Nova senha (Esqueci minha senha): campo de senha, sem tirar espaços. */
  secret?: boolean
  placeholder: string
  /** Seis dígitos (autenticador, e-mail); falso no código de recuperação. */
  numeric: boolean
  action: string
  busy: boolean
  disabled: boolean
  onSubmit: (code: string) => void
}) {
  const field = useRef<HTMLInputElement>(null)
  const [filled, setFilled] = useState(false)
  useEffect(() => {
    if (!disabled) return
    if (field.current) field.current.value = ''
    setFilled(false)
  }, [disabled])
  useEffect(() => {
    const node = field.current
    return () => {
      if (node) node.value = ''
    }
  }, [])
  return (
    <form
      className={secret ? 'welcome-password is-secret' : 'welcome-password is-code'}
      aria-label={label}
      noValidate
      onSubmit={(event) => {
        event.preventDefault()
        const node = field.current
        if (disabled || !node) return
        const code = secret ? node.value : node.value.replace(/\s+/g, '')
        node.value = ''
        setFilled(false)
        if (code) onSubmit(code)
      }}
    >
      <input
        ref={field}
        className="welcome-field"
        type={secret ? 'password' : 'text'}
        aria-label={label}
        placeholder={placeholder}
        inputMode={numeric ? 'numeric' : 'text'}
        autoComplete={secret ? 'new-password' : 'one-time-code'}
        autoCapitalize="off"
        spellCheck={false}
        maxLength={numeric ? 6 : secret ? 4096 : 64}
        disabled={disabled}
        onChange={() => setFilled(!!field.current?.value)}
      />
      <button className="welcome-primary" type="submit" disabled={disabled || !filled}>
        {busy ? <BusyMark size={15} /> : null}
        {action}
      </button>
    </form>
  )
}
