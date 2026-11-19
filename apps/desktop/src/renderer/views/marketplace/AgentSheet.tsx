import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { Backend } from '@shared/models'
import { agentIconURL } from '../../components/AgentIcon'
import { Button, CardForm, CardSection, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, SheetPage } from '../../components/Overlay'
import { HStack, IconSlot, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { ThinkingOrb } from '../../components/ThinkingOrb'
import { useStore } from '../../store/context'
import { errorText, market, type AgentAuth, type AuthMethod } from './market-models'
import { ErrorText, Field, ScreenHeader, SlideStack, WebLink, filled } from './pieces'
import { SetupTerminalView } from '../fork/sob-demanda/telas'
import { RemoteEngineSheet } from '../fork/contas-ia/RemoteEngineSheet'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

type SetupRoute = { kind: 'install' } | { kind: 'node' } | { kind: 'terminal'; method: AuthMethod | null } | { kind: 'keys'; method: AuthMethod }

/** Registry logos are one-color; paint the ones with a known brand color in it. */
export function agentTint(registry: string | null | undefined) {
  switch (registry) {
    case 'claude-acp': return 'rgb(217, 120, 87)'
    case 'gemini': return 'linear-gradient(to top right, rgb(71, 150, 227), rgb(145, 120, 199), rgb(201, 102, 115))'
    case 'antigravity-acp': return 'rgb(66, 133, 245)'
    case 'mistral-vibe': return 'rgb(250, 82, 15)'
    case 'qwen-code': return 'rgb(97, 92, 237)'
    case 'amp-acp': return 'rgb(242, 79, 64)'
    case 'kiro': return 'rgb(143, 69, 255)'
    case 'cortex-code': return 'rgb(41, 181, 232)'
    case 'omp': return 'linear-gradient(to bottom right, rgb(237, 74, 191), rgb(155, 77, 255), rgb(90, 216, 230))'
    default: return 'var(--text-primary, var(--ink))'
  }
}

/** An agent's registry logo on a rounded tile. */
export function AgentTile({ registry, icon, tile, radius }: { registry?: string | null; icon: number; tile: number; radius: number }) {
  return (
    <div className="agent-tile" style={{ width: tile, height: tile, borderRadius: radius }}>
      <TintedAgentIcon registry={registry} size={icon} />
    </div>
  )
}

/** `AgentIcon` painted with `agentTint` (which can be a gradient, so a mask over `background`). */
function TintedAgentIcon({ registry, size }: { registry?: string | null; size: number }) {
  const url = agentIconURL(registry)
  if (!url) return <Icon name="terminal" size={size * 0.8} scaled={false} />
  return (
    <span
      aria-hidden
      className="agent-icon"
      style={{ width: size, height: size, background: agentTint(registry), WebkitMaskImage: `url("${url}")` }}
    />
  )
}

/**
 * Above the agents while some of them run on Node.js and the computer has none: one place to
 * add Codync's copy for all of them.
 */
export function NodeNotice({ onAdd }: { onAdd: () => void }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  return (
    <div className="node-notice fade-in idioma-superficie" {...superficieIdioma(idioma)}>
      <Icon name="shippingbox" size={15} color="var(--context-ink-2, var(--ink-2))" />
      <VStack as="span" gap={2} grow>
        <Text size="subheadline" weight="semibold" tone="text">{t('extensoes.motor.semNode', { computador: isolado(store.hostName) })}</Text>
        <Text size="caption" tone="secondary">{t('marca.node.copia')}</Text>
      </VStack>
      <Button onClick={onAdd}>{t('extensoes.motor.adicionarNode')}</Button>
    </div>
  )
}

/**
 * Getting an agent ready on the computer: install it, add Node.js when it runs on Node and
 * the computer has none, then sign in. Sign-in
 * options come from the agent itself (ACP), plus Codync's own command for
 * CLIs it knows. Bots are made from New bot, not here.
 */
/** `recheck`: opened from a chat's sign-in request, whose agent refused a turn, so the agent is
 *  asked again however signed in it looked. */
export function AgentSheet(props: { initial: Backend; recheck?: boolean; botId?: string }) {
  const store = useStore()
  if (!store.isLoopback && (props.initial.id === 'codex' || props.initial.id === 'claude')) {
    return <RemoteEngineSheet engine={props.initial.id} botId={props.botId} />
  }
  return <LegacyAgentSheet {...props} />
}

function LegacyAgentSheet({ initial, recheck = false }: { initial: Backend; recheck?: boolean }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const [route, setRoute] = useState<SetupRoute | null>(null)
  const [auth, setAuth] = useState<AgentAuth | null>(null)
  const [checking, setChecking] = useState(false)
  const [authenticating, setAuthenticating] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const checkingRef = useRef(false)
  // D169: fechada a folha (ou trocado o Hub, que a fecha em Ajustes), uma resposta tardia não mexe em mais nada.
  const alive = useRef(true)
  useEffect(() => () => { alive.current = false }, [])

  // Live: an install or sign-in updates the host's list.
  const backend = store.hello?.backends.find((b) => b.id === initial.id) ?? initial
  const installed = backend.installed === true
  const curated = backend.curated === true
  const signedIn = auth?.signedIn ?? backend.signedIn ?? null
  const nodeMissing = backend.node === 'missing'
  // O nome do computador e do motor são dados: entram isolados nas frases.
  const host = isolado(store.hostName)
  const motor = isolado(backend.name)

  const check = async () => {
    const client = store.client
    if (!client || checkingRef.current) return
    checkingRef.current = true
    setChecking(true)
    try {
      const next = await market.agentAuth(client, backend.id)
      if (!alive.current) return
      setAuth(next)
      setError(null)
    } catch (e) {
      if (!alive.current) return
      setError(errorText(e))
    }
    checkingRef.current = false
    setChecking(false)
    await store.refreshBackends()
  }

  const authenticate = async (m: AuthMethod) => {
    const client = store.client
    if (!client) return
    setAuthenticating(m.id)
    try {
      const next = await market.agentAuthenticate(client, backend.id, m.id)
      if (!alive.current) return
      setAuth(next)
      setError(null)
    } catch (e) {
      if (!alive.current) return
      setError(errorText(e))
    }
    setAuthenticating(null)
    await store.refreshBackends()
  }

  // A known "signed in" needs no agent start; everything else asks the agent (which can't
  // start without its Node.js).
  useEffect(() => {
    if ((recheck || backend.signedIn !== true) && !nodeMissing) void check()
  }, [])

  // Back from a terminal or key form: see what changed.
  const hadRoute = useRef(false)
  useEffect(() => {
    if (route === null && hadRoute.current) void check()
    hadRoute.current = route !== null
  }, [route])

  const back = () => setRoute(null)
  const routed = route === null ? null
    : route.kind === 'install' ? <SetupTerminalView backend={backend} step="install" back={back} />
    : route.kind === 'node' ? <SetupTerminalView backend={backend} step="node" back={back} />
    : route.kind === 'terminal' ? <SetupTerminalView backend={backend} step="login" method={route.method} back={back} />
    : <AgentKeysForm backend={backend} method={route.method} saved={auth?.savedEnv ?? []} back={back} done={setAuth} />

  const statusText = checking
    ? (auth === null ? t('extensoes.motor.conferindoPrimeira', { motor }) : t('extensoes.motor.conferindo'))
    : signedIn === true ? t('extensoes.motor.loginFeito')
    : nodeMissing ? t('extensoes.motor.nodePrimeiro')
    : signedIn === false ? t('extensoes.motor.semLogin')
    : t('extensoes.motor.naoSabe', { computador: host })

  const optionRow = (key: string, title: string, detail: string, icon: string, action: () => void, busy = false) => (
    <button key={key} className="option-row" disabled={authenticating !== null} onClick={action}>
      <IconSlot width={26} tone="secondary" aria-hidden>
        <Icon name={icon} size={13} />
      </IconSlot>
      <VStack as="span" gap={2} grow>
        <Text size="body" weight="medium" tone="text" dir="auto">{title}</Text>
        <Text size="subheadline" tone="secondary" lines={3} dir="auto">{detail}</Text>
      </VStack>
      <span className="agent-row-gap" />
      {busy ? <ThinkingOrb size={16} color="var(--context-ink-2, var(--ink-2))" /> : <Icon name="chevron.right" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />}
    </button>
  )

  const options: ReactNode[] = []
  // Some CLIs drop the current sign-in the moment a new one starts: no options once signed in.
  if (signedIn !== true && !checking) {
    if (auth?.login) options.push(optionRow('login', t('extensoes.motor.entrar'), t('extensoes.motor.terminalLinks', { computador: host }), 'terminal', () => setRoute({ kind: 'terminal', method: null })))
    for (const m of auth?.methods ?? []) {
      if (m.kind === 'terminal') options.push(optionRow(m.id, m.name, m.description ?? t('extensoes.motor.terminal', { computador: host }), 'terminal', () => setRoute({ kind: 'terminal', method: m })))
      else if (m.kind === 'envVar') options.push(optionRow(m.id, m.name, m.description ?? t('extensoes.motor.guardadaSo', { computador: host }), 'key', () => setRoute({ kind: 'keys', method: m })))
      else {
        options.push(optionRow(
          m.id,
          m.name,
          authenticating === m.id ? t('extensoes.motor.termineNavegador', { computador: host }) : (m.description ?? t('extensoes.motor.abreNavegador', { computador: host })),
          'safari',
          () => void authenticate(m),
          authenticating === m.id,
        ))
      }
    }
  }

  const overview = (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        icon={<AgentTile registry={backend.registry ?? backend.id} icon={18} tile={32} radius={9} />}
        title={<bdi dir="auto">{backend.name}</bdi>}
        subtitle={backend.description ? <bdi dir="auto">{backend.description}</bdi> : undefined}
        closeTitle={t('extensoes.fechar')}
      />
      <CardForm>
        {/* D164: contas do Codex e do Claude Code ficam em Configurações → Contas de IA. */}
        {backend.id === 'codex' || backend.id === 'claude' ? <CardSection><Text size="subheadline" tone="secondary">{t('contas.avisoCatalogo')}</Text></CardSection> : null}
        {curated ? (
          <CardSection>
            <SetupStepRow
              number={1}
              title={t('extensoes.motor.instalar')}
              detail={installed ? t('extensoes.motor.instalado', { computador: host }) : backend.canInstall ? t('extensoes.motor.instaladorOficial', { computador: host }) : backend.installHint}
              done={installed}
              action={installed || !backend.canInstall ? null : { label: t('extensoes.motor.instalar'), run: () => setRoute({ kind: 'install' }) }}
            />
          </CardSection>
        ) : null}
        {backend.node ? (
          <CardSection>
            <SetupStepRow
              number={curated ? 2 : 1}
              title={t('extensoes.motor.node')}
              detail={nodeMissing ? t('marca.node.roda', { motor }) : t('extensoes.motor.adicionadoNo', { computador: host })}
              done={!nodeMissing}
              action={nodeMissing ? { label: t('extensoes.motor.adicionar'), run: () => setRoute({ kind: 'node' }) } : null}
            />
          </CardSection>
        ) : null}
        <CardSection
          footer={signedIn !== true && auth?.methods.some((m) => m.kind === 'agent')
            ? t('extensoes.motor.navegadorNoHub', { computador: host })
            : null}
        >
          <HStack>
            <VStack grow>
              <SetupStepRow number={1 + Number(curated) + Number(backend.node !== undefined)} title={t('extensoes.motor.entrar')} detail={statusText} done={signedIn === true} action={null} />
            </VStack>
            {checking ? (
              <ThinkingOrb state="connecting" size={16} color="var(--context-ink-2, var(--ink-2))" />
            ) : (
              <button title={t('extensoes.motor.conferirDeNovo')} aria-label={t('extensoes.motor.conferirDeNovo')} className="agent-check-again" onClick={() => void check()}>
                <Icon name="arrow.clockwise" size={13} />
              </button>
            )}
          </HStack>
          {signedIn !== true && auth?.detail ? <Text size="footnote" tone="secondary" className="selectable agent-auth-detail">{auth.detail}</Text> : null}
          {options}
        </CardSection>
        {error ? <CardSection><ErrorText text={error} /></CardSection> : null}
      </CardForm>
      </div>
    </SheetPage>
  )

  return <SlideStack base={overview} top={routed} />
}

/** Keys an agent reads from its environment, kept on the computer. */
function AgentKeysForm({ backend, method, saved, back, done }: {
  backend: Backend
  method: AuthMethod
  saved: string[]
  back: () => void
  done: (auth: AgentAuth) => void
}) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const [values, setValues] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const vars = method.vars ?? []

  const save = async (vars: Record<string, string>) => {
    const client = store.client
    if (!client) return
    setSaving(true)
    try {
      done(await market.setAgentEnv(client, backend.id, vars))
      back()
    } catch (e) {
      setError(errorText(e))
    }
    setSaving(false)
  }

  const missing = vars.some((v) => !v.optional && !values[v.name] && !saved.includes(v.name))
  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ScreenHeader
        title={method.name}
        onBack={back}
        trailing={saving ? <Spinner /> : <IconButton title={t('extensoes.motor.salvar')} icon="checkmark" disabled={missing} onClick={() => void save(filled(values))} />}
      />
      <CardForm>
        <VStack gap={6}>
          <CardSection title={method.name}>
            {vars.map((v) => (
              <VStack key={v.name} gap={4}>
                <Field
                  label={v.label}
                  value={values[v.name] ?? ''}
                  secret={v.secret}
                  placeholder={saved.includes(v.name) ? t('extensoes.motor.guardadaTrocar') : v.name}
                  onChange={(x) => setValues((old) => ({ ...old, [v.name]: x }))}
                />
                <Text size="caption" tone="secondary" dir="auto">{v.optional ? t('extensoes.motor.opcional', { rotulo: isolado(v.label) }) : v.label}</Text>
              </VStack>
            ))}
          </CardSection>
          <VStack gap={6} className="agent-keys-notes">
            {method.description ? <span>{method.description}</span> : null}
            <span>{t('extensoes.motor.entregue', { computador: isolado(store.hostName), motor: isolado(backend.name) })}</span>
            {method.link && URL.canParse(method.link) ? <WebLink title={t('extensoes.motor.obterChave')} url={method.link} /> : null}
          </VStack>
        </VStack>
        {saved.length ? (
          <CardSection>
            <button className="market-danger-button" onClick={() => void save(Object.fromEntries(vars.map((v) => [v.name, ''])))}>
              {t('extensoes.motor.removerChaves')}
            </button>
          </CardSection>
        ) : null}
        {error ? <CardSection><ErrorText text={error} /></CardSection> : null}
      </CardForm>
      </div>
    </SheetPage>
  )
}

function SetupStepRow({ number, title, detail, done, action }: {
  number: number
  title: string
  detail: string
  done: boolean
  action: { label: string; run: () => void } | null
}) {
  const t = useT()
  return (
    <HStack gap={12} className="agent-step-row" aria-label={done ? t('extensoes.motor.passoFeito', { titulo: title, detalhe: detail }) : t('extensoes.motor.passo', { titulo: title, detalhe: detail })}>
      <span aria-hidden className={`agent-step-number${done ? ' done' : ''}`}>
        {done
          ? <Icon name="checkmark" size={10} weight="bold" color="var(--on-accent)" />
          : <Text size="caption" weight="semibold" tone="secondary">{number}</Text>}
      </span>
      <VStack as="span" gap={2} grow>
        <Text size="body" weight="medium" tone="text">{title}</Text>
        <Text size="subheadline" tone="secondary" className="selectable">{detail}</Text>
      </VStack>
      <span className="agent-row-gap" />
      {action ? (
        <button className="capsule-action agent-step-action" onClick={action.run}>
          {action.label}
        </button>
      ) : null}
    </HStack>
  )
}
