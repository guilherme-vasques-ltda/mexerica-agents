// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useState } from 'react'
import { isGroup } from '@shared/models'
import { Button, CardForm, CardSection, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, SheetPage, useDismiss } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { errorText, loginTitle, market, type CredentialStatus, type InstalledConnector, type SavedLogin } from './market-models'
import { ErrorText, Field, FormSheet, ScreenHeader, filled } from './pieces'
import { usePlugins } from './plugins'
import { LogoConector } from '../fork/extensoes/LogoConector'
import './credentials.css'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

// Exemplos nos campos de login: dado técnico, igual em todo idioma e da esquerda para a direita.
const EXEMPLO_SITE = 'github.com' // textos-ok: exemplo de domínio, igual em todo idioma
const EXEMPLO_EMAIL = 'name@example.com' // textos-ok: exemplo de e-mail, igual em todo idioma
const ONE_PASSWORD = '1Password' // textos-ok: nome próprio do serviço, igual em todo idioma

/**
 * `embedded` (revisão do app, Entrega 2): a página Credenciais da tela de Extensões, sem o cabeçalho de folha (o
 * título é o da página, e Voltar/Fechar são os da tela).
 */
export function CredentialsView({ onBack, embedded = false }: { onBack?: () => void; embedded?: boolean }) {
  const dismiss = useDismiss()
  const t = useT()
  const idioma = useIdioma()
  const store = useStore()
  const plugins = usePlugins(store)
  const [selected, setSelected] = useState<InstalledConnector | null>(null)
  const [token, setToken] = useState('')
  const [status, setStatus] = useState<CredentialStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      await plugins.refresh()
      const client = store.client
      if (!client) return
      try {
        setStatus(await market.credentialStatus(client))
      } catch (e) {
        setError(errorText(e))
      }
    })()
  }, [plugins, store])

  const save = async (value: string) => {
    const client = store.client
    if (!client) return
    setBusy(true)
    setError(null)
    try {
      setStatus(await market.setOnePasswordToken(client, value))
      setToken('')
    } catch (e) {
      setError(errorText(e))
    }
    setBusy(false)
  }

  return (
    <SheetPage className={`credentials-page${embedded ? ' credentials-embedded' : ''}`}>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      {embedded ? null : <ScreenHeader title={t('extensoes.credenciais')} onBack={onBack ?? dismiss} trailing={<IconButton title={t('extensoes.fechar')} icon="xmark" onClick={dismiss} />} />}
      <CardForm>
        <CardSection title={t('extensoes.cred.armazenamento')}>
          <div className="credential-storage"><Icon name="lock.shield" size={20} /><div><strong dir={status?.provider ? 'auto' : undefined}>{status?.provider ?? t('extensoes.cred.cofreSistema')}</strong><p>{t('extensoes.cred.cifrado')}</p></div></div>
        </CardSection>
        <CardSection title={t('extensoes.cred.conexoesSalvas')}>
          {plugins.installedConnectors.filter((c) => c.keys.length > 0).map((c) => (
            <button key={c.id} className="plain-row-button" onClick={() => setSelected(c)}>
              <LogoConector conector={c} size={26} />
              <span className="credential-row-title" dir="auto">{c.name}</span>
              <Icon name="chevron.right" size={10} />
            </button>
          ))}
          {!plugins.installedConnectors.some((c) => c.keys.length > 0) ? <Text size="caption" tone="secondary">{t('extensoes.cred.nenhumaSalva')}</Text> : null}
          <Text size="caption" tone="secondary">{t('extensoes.cred.valoresOcultos')}</Text>
        </CardSection>
        <LoginsSection />
        <CardSection title={ONE_PASSWORD} footer={t('extensoes.cred.opRodape')}>
          <span>{status?.onePasswordConnected ? t('extensoes.cred.opConectado') : t('extensoes.cred.opConectar')}</span>
          <CredentialField label={t('extensoes.cred.tokenServico')} value={token} onChange={setToken} placeholder={t('extensoes.cred.colarToken')} secret />
          <div><Button disabled={busy || !token} onClick={() => void save(token)}>{t('extensoes.cred.conectar')}</Button></div>
          {status?.onePasswordConnected ? <div><Button kind="secondary" disabled={busy} onClick={() => void save('')}>{t('extensoes.cred.desconectar')}</Button></div> : null}
          <Text size="footnote" tone="secondary">{t('marca.credenciais.op')}</Text>
        </CardSection>
        {busy ? <Spinner /> : null}
        {error ? <ErrorText text={error} /> : null}
      </CardForm>
      <FormSheet language={superficieIdioma(idioma)} open={selected !== null} onClose={() => setSelected(null)}>
        {selected ? <ConnectorCredentialEditor key={selected.id} connector={selected} /> : null}
      </FormSheet>
      </div>
    </SheetPage>
  )
}

/**
 * Website and app logins bots type through the computer tool; passwords stay hidden. Each is an account
 * with a label, turned on bot by bot in the bot's settings (D82).
 */
function LoginsSection() {
  const store = useStore()
  const t = useT()
  const [logins, setLogins] = useState<SavedLogin[]>([])
  const [site, setSite] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [label, setLabel] = useState('')
  const [renaming, setRenaming] = useState<{ id: string; label: string } | null>(null)
  // Auditoria (2ª rodada): o formulário de novo login fica recolhido atrás de "Adicionar login".
  const [adding, setAdding] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Hidden bots too: they can have logins on.
  const bots = [...store.bots.values()].filter((b) => !isGroup(b))
  /** The account under a custom label, and which bots have it on. */
  const detail = (login: SavedLogin) => {
    const account = login.username ? `${login.site} · ${login.username}` : login.site
    const on = bots.filter((b) => b.logins.includes(login.id)).map((b) => isolado(b.name))
    const usage = on.length ? t('extensoes.cred.ligadoPara', { bots: on.join(', ') }) : t('extensoes.cred.nenhumBot')
    return loginTitle(login) === account ? usage : t('extensoes.cred.contaUso', { conta: `\u2066${account}\u2069`, uso: usage })
  }

  const load = async () => {
    const client = store.client
    try {
      setLogins(client ? await market.logins(client) : [])
    } catch (e) {
      setError(errorText(e))
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const save = async () => {
    const client = store.client
    if (!client) return
    setBusy(true)
    setError(null)
    try {
      await market.saveLogin(client, site, username, password, label)
      setSite('')
      setUsername('')
      setPassword('')
      setLabel('')
      setAdding(false)
      await load()
    } catch (e) {
      setError(errorText(e))
    }
    setBusy(false)
  }

  const rename = async () => {
    const client = store.client
    if (!client || !renaming) return
    setError(null)
    try {
      await market.labelLogin(client, renaming.id, renaming.label)
      setRenaming(null)
      await load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const remove = async (login: SavedLogin) => {
    const client = store.client
    if (!client) return
    try {
      await market.removeLogin(client, login.id)
      setLogins((old) => old.filter((l) => l.id !== login.id))
    } catch (e) {
      setError(errorText(e))
    }
  }

  return (
    <CardSection
      title={t('extensoes.cred.logins')}
      footer={t('extensoes.cred.loginsRodape')}
    >
      {logins.map((login) => renaming?.id === login.id ? (
        <HStack key={login.id} gap={6}>
          <div className="credential-rename"><Field value={renaming.label} onChange={(v) => setRenaming({ id: login.id, label: v })} placeholder={login.username ? `${login.site} · ${login.username}` : login.site} /></div>
          <IconButton title={t('extensoes.cred.salvarRotulo')} icon="checkmark" onClick={() => void rename()} />
          <IconButton title={t('extensoes.cred.cancelar')} icon="xmark" onClick={() => setRenaming(null)} />
        </HStack>
      ) : (
        <HStack key={login.id}>
          <VStack gap={2} grow>
            <span dir="auto">{loginTitle(login)}</span>
            <Text size="caption" tone="secondary">{detail(login)}</Text>
          </VStack>
          <IconButton title={t('extensoes.cred.renomear')} icon="pencil" onClick={() => setRenaming({ id: login.id, label: loginTitle(login) })} />
          <IconButton title={t('extensoes.cred.remover')} icon="trash" onClick={() => void remove(login)} />
        </HStack>
      ))}
      {adding ? <CredentialField label={t('extensoes.cred.site')} value={site} onChange={setSite} placeholder={EXEMPLO_SITE} ltr /> : null}
      {adding ? <CredentialField label={t('extensoes.cred.usuario')} value={username} onChange={setUsername} placeholder={EXEMPLO_EMAIL} ltr /> : null}
      {adding ? <CredentialField label={t('extensoes.cred.senha')} value={password} onChange={setPassword} placeholder={t('extensoes.cred.senhaOuOp')} secret /> : null}
      {adding ? <CredentialField label={t('extensoes.cred.rotuloOpcional')} value={label} onChange={setLabel} placeholder={t('extensoes.cred.rotuloExemplo')} /> : null}
      {adding ? <div className="credential-adicionar"><Button disabled={busy || !site || !password} onClick={() => void save()}>{t('extensoes.cred.salvarLogin')}</Button><Button kind="secondary" onClick={() => setAdding(false)}>{t('extensoes.cred.cancelar')}</Button></div> : null}
      {adding ? null : <div><Button kind="secondary" onClick={() => setAdding(true)}><Icon name="plus" size={11} />{t('extensoes.cred.adicionarLogin')}</Button></div>}
      {error ? <Text size="footnote" tone="danger">{error}</Text> : null}
    </CardSection>
  )
}

function ConnectorCredentialEditor({ connector }: { connector: InstalledConnector }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const dismiss = useDismiss()
  const [fields, setFields] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const save = async () => {
    const client = store.client
    if (!client) return
    setSaving(true)
    setError(null)
    try {
      await market.updateConnectorCredentials(client, connector.id, filled(fields))
      setFields({})
      dismiss()
    } catch (e) {
      setError(errorText(e))
    }
    setSaving(false)
  }

  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        icon="key"
        title={<bdi dir="auto">{connector.name}</bdi>}
        closeTitle={t('extensoes.fechar')}
        trailing={saving ? <Spinner /> : <IconButton title={t('extensoes.cred.salvarCredenciais')} icon="checkmark" disabled={Object.values(fields).every((v) => !v)} onClick={() => void save()} />}
      />
      <CardForm>
        <CardSection footer={t('extensoes.cred.soTrocar')}>
          {connector.keys.map((key) => (
            <Field key={key} label={key} value={fields[key] ?? ''} placeholder={key} secret onChange={(v) => setFields((old) => ({ ...old, [key]: v }))} />
          ))}
        </CardSection>
        {error ? <ErrorText text={error} /> : null}
      </CardForm>
      </div>
    </SheetPage>
  )
}

/** Persistent labels keep a filled credential field identifiable. */
function CredentialField({ label, ...props }: Parameters<typeof Field>[0] & { label: string }) {
  return <label className="credential-field"><span>{label}</span><Field {...props} label={label} /></label>
}
