import { useEffect, useRef, useState, type ReactNode } from 'react'
import QRCode from 'qrcode'
import { canUnlink, passwordProblem, type AccountProfile, type ProfileResult, type Reverification } from '@shared/fork-account-profile'
import type { SignInProvider } from '@shared/analytics'
import { Button, CardSection, Spinner } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import googleLogo from '../../../assets/google.svg'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { ltr } from '../../../../shared/bidi.ts'
import './account-profile.css'

/** DON-917 (estado é dado): um problema desta tela é chave do catálogo; o que vem do processo principal fica cru. */
type Problema = { chave: ChaveTexto } | { texto: string } | null
const textoDo = (t: ReturnType<typeof useT>, p: Problema) => (!p ? null : 'chave' in p ? (t as unknown as (c: ChaveTexto) => string)(p.chave) : p.texto)

/**
 * E6b (DON-864): Ajustes → Conta, abaixo da linha da conta. Nome, senha, contas ligadas e
 * verificação em duas etapas, tudo pelo processo principal (`window.codync.accountProfile`).
 * Senhas e códigos ficam só nos campos e no estado desta tela, que some ao sair dela.
 */
const bridge = () => window.codync.accountProfile
const NAMES = { google: 'conta.provedor.google', apple: 'conta.provedor.apple', x: 'conta.provedor.x' } as const satisfies Record<SignInProvider, ChaveTexto>
export const ACCOUNT_CONTACT = 'contato@example.invalid'

type Run = <T>(work: () => Promise<ProfileResult<T>>, done: (value: T) => void, fail?: (message: string) => void) => Promise<boolean>

export function AccountProfileSections() {
  const t = useT()
  const idioma = useIdioma()
  const [profile, setProfile] = useState<AccountProfile | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [reverify, setReverify] = useState<(() => void) | null>(null)
  useEffect(() => { void bridge().load().then(r => r.ok ? setProfile(r.value) : setError(r.message)) }, [])

  const run: Run = async (work, done, fail) => {
    setBusy(true); setError(null)
    try {
      const result = await work()
      if (result.ok) { done(result.value); return true }
      // O Clerk pede confirmar a identidade: depois dela, o mesmo gesto é refeito uma vez.
      if (result.failure === 'reverify') setReverify(() => () => { setReverify(null); void run(work, done, fail) })
      else if (fail) fail(result.message)
      else setError(result.message)
      return false
    } finally { setBusy(false) }
  }

  if (!profile) {
    return (
      <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection title={t('conta.perfil.titulo')} footer={error ? <span dir="auto">{error}</span> : null}>
        <div className="account-profile-row">{error ? <Text tone="secondary">{t('conta.perfil.erroCarregar')}</Text> : <Spinner size={16} />}</div>
      </CardSection>
      </div>
    )
  }
  const shared = { profile, busy, run, setProfile }
  return (
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      {reverify ? <ReverifyPanel onDone={reverify} onCancel={() => setReverify(null)} /> : null}
      {error ? <p className="account-profile-error" role="alert" dir="auto">{error}</p> : null}
      <NameSection {...shared} />
      <PasswordSection {...shared} />
      <LinkedSection {...shared} />
      <TwoFactorSection {...shared} />
    </div>
  )
}

interface Shared { profile: AccountProfile; busy: boolean; run: Run; setProfile: (p: AccountProfile) => void }

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="account-profile-field"><span>{label}</span>{children}</label>
}

function NameSection({ profile, busy, run, setProfile }: Shared) {
  const [first, setFirst] = useState(profile.firstName)
  const [last, setLast] = useState(profile.lastName)
  const [saved, setSaved] = useState(false)
  const changed = first.trim() !== profile.firstName || last.trim() !== profile.lastName
  const t = useT()
  return (
    <CardSection title={t('conta.perfil.titulo')} footer={t('conta.perfil.rodapeContato', { contato: ltr(ACCOUNT_CONTACT) })}>
      <div className="account-profile-form">
        <div className="account-profile-pair">
          <Field label={t('conta.perfil.nome')}><input className="account-profile-input" dir="auto" value={first} maxLength={100} autoComplete="given-name" disabled={busy} onChange={e => { setFirst(e.target.value); setSaved(false) }} /></Field>
          <Field label={t('conta.perfil.sobrenome')}><input className="account-profile-input" dir="auto" value={last} maxLength={100} autoComplete="family-name" disabled={busy} onChange={e => { setLast(e.target.value); setSaved(false) }} /></Field>
        </div>
        <div className="account-profile-actions">
          {saved ? <span className="account-profile-note" role="status">{t('conta.perfil.nomeSalvo')}</span> : null}
          <Button kind="secondary" disabled={busy || !changed} onClick={() => void run(() => bridge().saveName(first, last), p => { setProfile(p); setFirst(p.firstName); setLast(p.lastName); setSaved(true) })}>{t('conta.perfil.salvarNome')}</Button>
        </div>
      </div>
      <div className="account-profile-row">
        <div className="account-profile-text"><Text>{t('conta.perfil.email')}</Text><Text size="caption" tone="secondary" dir="ltr">{profile.email ?? t('conta.perfil.semEmail')}</Text></div>
      </div>
    </CardSection>
  )
}

function PasswordSection({ profile, busy, run, setProfile }: Shared) {
  const [open, setOpen] = useState(false)
  const [done, setDone] = useState(false)
  const current = useRef<HTMLInputElement>(null)
  const next = useRef<HTMLInputElement>(null)
  const again = useRef<HTMLInputElement>(null)
  const [problem, setProblem] = useState<Problema>(null)
  const t = useT()
  const rules = profile.passwordRules
  const creating = !profile.hasPassword
  const submit = () => {
    const now = current.current?.value ?? '', value = next.current?.value ?? '', repeat = again.current?.value ?? ''
    // A regra da senha vem do `shared` em pt-BR (fatia 10); fica crua até lá.
    const regra = passwordProblem(value, rules)
    const local: Problema = !creating && !now ? { chave: 'conta.senha.digiteAtual' } : regra ? { texto: regra } : value !== repeat ? { chave: 'conta.senha.diferentes' } : null
    setProblem(local)
    if (local) return
    void run(() => bridge().setPassword(creating ? null : now, value), p => { setProfile(p); setOpen(false); setDone(true) }, message => {
      setProblem({ texto: message })
      if (current.current) current.current.value = ''
    })
  }
  // A senha some dos campos quando o formulário fecha.
  useEffect(() => { if (!open) for (const r of [current, next, again]) if (r.current) r.current.value = '' }, [open])
  return (
    <CardSection title={t('conta.senha.titulo')}>
      <div className="account-profile-row">
        <div className="account-profile-text">
          <Text>{t(creating ? 'conta.senha.semSenha' : 'conta.senha.definida')}</Text>
          <Text size="caption" tone="secondary">{t(done ? (creating ? 'conta.senha.criada' : 'conta.senha.trocada') : creating ? 'conta.senha.soGoogle' : 'conta.senha.usarSenha')}</Text>
        </div>
        {!open ? <Button kind="secondary" disabled={busy} onClick={() => { setOpen(true); setDone(false); setProblem(null) }}>{t(creating ? 'conta.senha.criar' : 'conta.senha.trocar')}</Button> : null}
      </div>
      {open ? (
        <form className="account-profile-form" onSubmit={e => { e.preventDefault(); submit() }}>
          <input type="email" autoComplete="username" value={profile.email ?? ''} readOnly hidden />
          {!creating ? <Field label={t('conta.senha.atual')}><input ref={current} className="account-profile-input" type="password" autoComplete="current-password" disabled={busy} /></Field> : null}
          <Field label={t('conta.senha.nova')}><input ref={next} className="account-profile-input" type="password" autoComplete="new-password" disabled={busy} /></Field>
          <Field label={t('conta.senha.repita')}><input ref={again} className="account-profile-input" type="password" autoComplete="new-password" disabled={busy} /></Field>
          <span className={problem ? 'account-profile-problem' : 'account-profile-note'} role={problem ? 'alert' : undefined} dir={problem && 'texto' in problem ? 'auto' : undefined}>{textoDo(t, problem) ?? t('conta.senha.minimo', { n: rules?.minLength ?? 8 })}</span>
          <div className="account-profile-actions">
            <Button kind="secondary" disabled={busy} onClick={() => setOpen(false)}>{t('conta.cancelar')}</Button>
            <button className="styled-button primary" type="submit" disabled={busy}>{t(creating ? 'conta.senha.criar' : 'conta.senha.salvar')}</button>
          </div>
        </form>
      ) : null}
    </CardSection>
  )
}

function ProviderMark({ provider }: { provider: SignInProvider }) {
  // textos-ok: glifo decorativo da marca X (aria-hidden), não é texto.
  const glifo = provider === 'x' ? '𝕏' : ''
  return provider === 'google' ? <img src={googleLogo} alt="" width={20} height={20} /> : <span className="account-profile-mark" aria-hidden>{glifo}</span>
}

function LinkedSection({ profile, busy, run, setProfile }: Shared) {
  const [confirm, setConfirm] = useState<string | null>(null)
  const t = useT()
  const rows = [
    ...profile.linked.map(a => ({ key: a.id, provider: a.provider, account: a })),
    ...profile.linkable.filter(p => !profile.linked.some(a => a.provider === p)).map(p => ({ key: p, provider: p, account: null })),
  ]
  if (!rows.length) return null
  return (
    <CardSection title={t('conta.ligadas.titulo')} footer={t('conta.ligadas.rodape')}>
      {rows.map(({ key, provider, account }) => {
        const only = account ? !canUnlink(profile, account.id) : false
        return (
          <div key={key} className="account-profile-row">
            <ProviderMark provider={provider} />
            <div className="account-profile-text">
              <Text>{t(NAMES[provider])}</Text>
              <Text size="caption" tone={only ? 'attention' : 'secondary'}>
                {!account ? t('conta.ligadas.naoLigada') : only ? t('conta.ligadas.unico') : account.email ? <span dir="ltr">{account.email}</span> : t('conta.ligadas.ligada')}
              </Text>
            </div>
            {!account ? (
              <Button kind="secondary" disabled={busy} onClick={() => void run(() => bridge().link(provider), setProfile)}>{t('conta.ligadas.ligar')}</Button>
            ) : confirm === account.id ? (
              <>
                <Button kind="secondary" disabled={busy} onClick={() => setConfirm(null)}>{t('conta.cancelar')}</Button>
                <Button kind="secondary" disabled={busy} onClick={() => void run(() => bridge().unlink(account.id), p => { setProfile(p); setConfirm(null) })}>{t('conta.ligadas.desligarProvedor', { provedor: t(NAMES[provider]) })}</Button>
              </>
            ) : (
              <Button kind="secondary" disabled={busy || only} onClick={() => setConfirm(account.id)}>{t('conta.ligadas.desligar')}</Button>
            )}
          </div>
        )
      })}
    </CardSection>
  )
}

type TwoFactorStep = { kind: 'idle' } | { kind: 'qr'; uri: string; secret: string; image: string | null } | { kind: 'codes'; codes: string[] } | { kind: 'disable' }

function TwoFactorSection({ profile, busy, run, setProfile }: Shared) {
  const [step, setStep] = useState<TwoFactorStep>({ kind: 'idle' })
  const [code, setCode] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const t = useT()
  const start = () => void run(() => bridge().startTotp(), async setup => {
    setCode(''); setProblem(null)
    setStep({ ...setup, kind: 'qr', image: null })
    const image = await QRCode.toDataURL(setup.uri, { errorCorrectionLevel: 'M', margin: 1, width: 360, color: { dark: '#000000', light: '#ffffff' } }).catch(() => null)
    setStep(s => s.kind === 'qr' && s.uri === setup.uri ? { ...s, image } : s)
  })
  const confirm = () => {
    setProblem(null)
    void run(() => bridge().confirmTotp(code), v => { setCode(''); setProfile({ ...profile, totp: true, backupCodes: true }); setStep({ kind: 'codes', codes: v.codes }) }, setProblem)
  }
  // Os códigos saem da tela ao concluir; o perfil é relido do Clerk.
  const finish = () => { setStep({ kind: 'idle' }); void bridge().load().then(r => r.ok && setProfile(r.value)) }
  return (
    <CardSection title={t('conta.dois.titulo')} footer={t('conta.dois.rodape')}>
      <div className="account-profile-row">
        <div className="account-profile-text">
          <Text>{t(profile.totp ? 'conta.dois.ligada' : 'conta.dois.desligada')}</Text>
          <Text size="caption" tone="secondary">{t(profile.totp ? 'conta.dois.detalheLigada' : 'conta.dois.detalheDesligada')}</Text>
        </div>
        {step.kind === 'idle' ? (
          profile.totp
            ? <Button kind="secondary" disabled={busy} onClick={() => setStep({ kind: 'disable' })}>{t('conta.dois.desligar')}</Button>
            : <Button kind="secondary" disabled={busy} onClick={start}>{t('conta.dois.ligar')}</Button>
        ) : null}
      </div>
      {step.kind === 'qr' ? (
        <form className="account-profile-form" onSubmit={e => { e.preventDefault(); confirm() }}>
          <ol className="account-profile-steps">
            <li>{t('conta.dois.passo1')}</li>
            <li>{t('conta.dois.passo2')}</li>
          </ol>
          <div className="account-profile-qr">
            {step.image ? <img src={step.image} alt={t('conta.dois.qrAlt')} width={160} height={160} /> : <span className="account-profile-qr-wait"><Spinner size={18} /></span>}
            <div className="account-profile-text">
              <Text size="caption" tone="secondary">{t('conta.dois.semCamera')}</Text>
              <code className="account-profile-secret" dir="ltr" aria-label={t('conta.dois.segredoRotulo')}>{step.secret.replace(/(.{4})/g, '$1 ').trim()}</code>
            </div>
          </div>
          <Field label={t('conta.dois.codigoApp')}><input className="account-profile-input account-profile-code" dir="ltr" inputMode="numeric" autoComplete="one-time-code" maxLength={7} value={code} disabled={busy} onChange={e => { setCode(e.target.value.replace(/[^\d ]/g, '')); setProblem(null) }} /></Field>
          {problem ? <span className="account-profile-problem" role="alert" dir="auto">{problem}</span> : null}
          <div className="account-profile-actions">
            <Button kind="secondary" disabled={busy} onClick={() => setStep({ kind: 'idle' })}>{t('conta.cancelar')}</Button>
            <button className="styled-button primary" type="submit" disabled={busy || code.replace(/\s/g, '').length !== 6}>{t('conta.confirmar')}</button>
          </div>
        </form>
      ) : null}
      {step.kind === 'codes' ? <RecoveryCodes codes={step.codes} onDone={finish} /> : null}
      {step.kind === 'disable' ? (
        <div className="account-profile-form">
          <Text>{t('conta.dois.desligarPergunta')}</Text>
          <Text size="caption" tone="secondary">{t('conta.dois.desligarAviso')}</Text>
          <div className="account-profile-actions">
            <Button kind="secondary" disabled={busy} onClick={() => setStep({ kind: 'idle' })}>{t('conta.cancelar')}</Button>
            <Button kind="primary" disabled={busy} onClick={() => void run(() => bridge().disableTotp(), p => { setProfile(p); setStep({ kind: 'idle' }) })}>{t('conta.dois.desligar')}</Button>
          </div>
        </div>
      ) : null}
      {profile.totp && step.kind === 'idle' ? (
        <div className="account-profile-row">
          <div className="account-profile-text">
            <Text>{t('conta.dois.recuperacao')}</Text>
            <Text size="caption" tone="secondary">{t('conta.dois.recuperacaoDetalhe')}</Text>
          </div>
          <Button kind="secondary" disabled={busy} onClick={() => void run(() => bridge().regenerateCodes(), v => setStep({ kind: 'codes', codes: v.codes }))}>{t('conta.dois.gerarNovos')}</Button>
        </div>
      ) : null}
    </CardSection>
  )
}

/** Mostrados uma vez: só neste estado da tela, nunca guardados. */
export function RecoveryCodes({ codes, onDone }: { codes: string[]; onDone: () => void }) {
  const [kept, setKept] = useState(false)
  const [note, setNote] = useState<ChaveTexto | null>(null)
  const t = useT()
  const text = codes.join('\n')
  return (
    <div className="account-profile-form">
      <Text weight="semibold">{t('conta.codigos.titulo')}</Text>
      <Text size="caption" tone="secondary">{t('conta.codigos.detalhe')}</Text>
      <ul className="account-profile-codes" dir="ltr" aria-label={t('conta.dois.recuperacao')}>
        {codes.map(c => <li key={c}><code>{c}</code></li>)}
      </ul>
      <div className="account-profile-actions start">
        <Button kind="secondary" onClick={() => void navigator.clipboard?.writeText(text).then(() => setNote('conta.codigos.copiados'), () => setNote('conta.codigos.naoCopiou'))}>{t('conta.copiar')}</Button>
        <Button kind="secondary" onClick={() => void bridge().saveCodes(codes).then(r => setNote(r === 'saved' ? 'conta.codigos.salvo' : r === 'error' ? 'conta.codigos.naoSalvou' : null))}>{t('conta.codigos.baixar')}</Button>
        {note ? <span className="account-profile-note" role="status">{(t as unknown as (c: ChaveTexto) => string)(note)}</span> : null}
      </div>
      <label className="account-profile-check"><input type="checkbox" checked={kept} onChange={e => setKept(e.target.checked)} /> {t('conta.codigos.guardei')}</label>
      <div className="account-profile-actions">
        <Button kind="primary" disabled={!kept} onClick={onDone}>{t('conta.concluir')}</Button>
      </div>
    </div>
  )
}

/** O Clerk pede confirmar a identidade antes de uma mudança sensível (sessão antiga). */
function ReverifyPanel({ onDone, onCancel }: { onDone: () => void; onCancel: () => void }) {
  const [state, setState] = useState<Reverification | null>(null)
  const [factor, setFactor] = useState<Reverification['factors'][number] | null>(null)
  const [sent, setSent] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const t = useT()
  const field = useRef<HTMLInputElement>(null)
  const apply = (r: ProfileResult<Reverification>) => {
    if (!r.ok) { setProblem(r.message); return }
    if (r.value.status === 'complete') { onDone(); return }
    setState(r.value); setFactor(f => f && r.value.factors.includes(f) ? f : r.value.factors[0] ?? null)
    if (field.current) field.current.value = ''
  }
  useEffect(() => { void bridge().startReverify().then(apply) }, [])
  const submit = async () => {
    const value = field.current?.value ?? ''
    if (!factor || !value) return
    setBusy(true); setProblem(null)
    try { apply(await bridge().attemptReverify(factor === 'password' ? { strategy: 'password', password: value } : { strategy: factor, code: value })) }
    finally { setBusy(false); if (field.current) field.current.value = '' }
  }
  const label = t(factor === 'password' ? 'conta.reverificar.fator.password' : factor === 'email_code' ? 'conta.reverificar.fator.email_code' : factor === 'backup_code' ? 'conta.reverificar.fator.backup_code' : 'conta.reverificar.fator.totp')
  return (
    <CardSection title={t('conta.reverificar.titulo')} footer={problem ? <span dir="auto">{problem}</span> : null}>
      <form className="account-profile-form" onSubmit={e => { e.preventDefault(); void submit() }}>
        <Text size="caption" tone="secondary">{t('conta.reverificar.motivo')}</Text>
        {!state ? <Spinner size={16} /> : !factor ? <Text tone="secondary">{t('conta.reverificar.semFator')}</Text> : (
          <>
            {factor === 'email_code' && !sent ? (
              <Button kind="secondary" disabled={busy} onClick={() => { setBusy(true); void bridge().sendReverifyCode().then(r => { setSent(r.ok); apply(r) }).finally(() => setBusy(false)) }}>{t('conta.reverificar.enviarEmail')}</Button>
            ) : (
              <Field label={label}><input ref={field} className="account-profile-input" type={factor === 'password' ? 'password' : 'text'} dir={factor === 'password' ? undefined : 'ltr'} autoComplete={factor === 'password' ? 'current-password' : 'one-time-code'} disabled={busy} /></Field>
            )}
            {state.factors.length > 1 ? (
              <div className="account-profile-actions start">
                {state.factors.filter(f => f !== factor).map(f => (
                  <button key={f} type="button" className="account-profile-link" onClick={() => { setFactor(f); setProblem(null) }}>
                    {t(f === 'password' ? 'conta.reverificar.usar.password' : f === 'email_code' ? 'conta.reverificar.usar.email_code' : f === 'backup_code' ? 'conta.reverificar.usar.backup_code' : 'conta.reverificar.usar.totp')}
                  </button>
                ))}
              </div>
            ) : null}
          </>
        )}
        <div className="account-profile-actions">
          <Button kind="secondary" disabled={busy} onClick={onCancel}>{t('conta.cancelar')}</Button>
          <button className="styled-button primary" type="submit" disabled={busy || !factor || (factor === 'email_code' && !sent)}>{t('conta.confirmar')}</button>
        </div>
      </form>
    </CardSection>
  )
}
