import { useEffect, useRef, useState, type FormEvent } from 'react'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import type { SetupOperation } from '../../../../shared/fork-engine-setup.ts'
import { Button, CardForm, IconButton } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { ThinkingOrb } from '../../../components/ThinkingOrb'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import { AgentTile } from '../../marketplace/AgentSheet'
import { Field, SlideStack } from '../../marketplace/pieces'
import type { ContasStore } from './contas-store'
import { accountView, clockText, engineName, engineRegistry, errorMessage, expiresAtMs, failureMessage, isFinal, providerUrl, stepOf } from './regras'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { SetupSession } from './setup-session'
import { TerminalDeSetup } from '../sob-demanda/telas'
import { useTextosContas } from './use-textos'
import type { TextosContas } from './textos'
import { superficieIdioma, tradutor, useIdioma } from '../../../lib/fork-i18n/estado'
import './contas-ia.css'

/**
 * Conectar ou reconectar uma conta de login (Codex/Claude Code) no Hub: Preparando →
 * Aguardando autorização → Verificando → Conectado, com cancelar e o terminal de setup como
 * alternativa. Fechar o painel não cancela; reabrir retoma a mesma operação.
 */
export function ConectarConta({ contas, account, reconnect, forBot, onKey, onClose }: {
  contas: ContasStore
  account: ManagedAccount
  reconnect: boolean
  /** Aberto pelo Motor de um bot: depois de conectar, oferece usar a conta nele. */
  forBot?: { name: string; use: (accountId: string) => void }
  /** A conta é de chave de API (o Hub recusou o login): seguir para o formulário seguro. */
  onKey: () => void
  onClose: () => void
}) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const store = useStore()
  const [session, setSession] = useState<SetupSession | null>(null)
  // A sessão nasce e morre com o painel (inclusive se o efeito rodar duas vezes): fechar apaga desafio e saída.
  useEffect(() => {
    // Preparar sempre vem antes: reaproveita o programa compatível ou instala o fixado, sem login.
    const created = new SetupSession(account.id, () => contas.account(account.id)?.revision ?? account.revision, () => store.ready(), engineSetup, {
      installFirst: true,
      onRef: (ref) => contas.rememberOperation(account.id, ref),
      onFinished: () => void contas.load(),
    })
    setSession(created)
    const ref = contas.operationFor(account.id)
    if (ref) created.resume(ref)
    else if (!reconnect) created.begin()
    return () => created.dispose()
  }, [store, contas, account.id])
  useModel(session)
  if (!session) return null

  const engine = account.backend
  const op = session.operation
  const phase = op?.phase ?? null
  const live = contas.account(account.id) ?? account
  const status = statusText(session, engine, textos, idioma)
  const title = reconnect ? textos.reconectarTitulo(account.label) : textos.conectarTitulo(account.label)

  const panel = (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ModalHeader
        icon={<AgentTile registry={engineRegistry(engine)} icon={18} tile={32} radius={9} />}
        title={title}
        subtitle={textos.ondeRoda(engineName(engine), store.hostName)}
        onClose={onClose}
        closeTitle={textos.fechar}
      />
      <div className="contas-panel-body">
        <CardForm>
        <VStack gap={18}>
          <Etapas session={session} />
          <p role="status" aria-live="polite" className="contas-status">{status}</p>
          {!op && !session.starting && session.reached < 0 ? (
            <VStack gap={8}>
              <Text tone="secondary">{reconnect && engine !== 'grok' ? textos.introReconectar : textos.introLogin(engineName(engine))}</Text>
              <Text size="caption" tone="secondary">{textos.introSemCusto}</Text>
              {engine === 'grok' ? <Text size="caption" tone="secondary">{tradutor(idioma)('contas.grokAviso')}</Text> : null}
            </VStack>
          ) : null}
          {op?.phase === 'awaiting_authorization' && op.challenge ? (
            <Desafio key={`${op.operationId}/${op.generation}`} session={session} operation={op} engine={engine} />
          ) : null}
          {phase === 'connected' ? <Conectada account={live} /> : null}
          {phase === 'failed' && op?.errorCode ? <Text size="caption" tone="tertiary" className="selectable">{tradutor(idioma)('contas.detalheTecnico', { detalhe: op.errorCode })}</Text> : null}
          {session.stalled && !session.error ? <Text size="caption" tone="attention">{textos.semResposta}</Text> : null}
          {session.error ? <Text tone="danger" className="selectable" role="alert">{errorMessage(session.error, idioma)}</Text> : null}
          {session.errorKind === 'api_form_required' ? (
            <HStack><Button onClick={onKey}>{textos.informarChave}</Button></HStack>
          ) : null}
        </VStack>
        </CardForm>
      </div>
      <Rodape session={session} reconnect={reconnect} forBot={forBot} onClose={onClose} accountId={account.id} />
    </div></SheetPage>
  )

  return <SlideStack base={panel} top={session.terminalOpen ? <TerminalDeSetup session={session} hostName={store.hostName} /> : null} />
}

function statusText(session: SetupSession, engine: ManagedAccount['backend'], textos: TextosContas, idioma: AppLanguage) {
  const op = session.operation
  if (session.cancelling && op && !isFinal(op.phase)) return textos.faseCancelando
  const installing = session.step === 'install'
  if (session.starting) return installing ? textos.faseInstalando(engineName(engine)) : textos.fasePreparando(engineName(engine))
  if (!op) return ''
  // A instalação inteira é Preparando; só o fim sem sucesso tem texto próprio.
  if (installing && (op.phase === 'preparing' || op.phase === 'verifying' || op.phase === 'connected' || op.phase === 'awaiting_authorization')) return textos.faseInstalando(engineName(engine))
  switch (op.phase) {
    case 'preparing': return textos.fasePreparando(engineName(engine))
    case 'awaiting_authorization': return session.overdue ? textos.faseExpirado : textos.faseAguardando
    case 'verifying': return textos.faseVerificando
    case 'connected': return textos.faseConectado
    case 'cancelled': return textos.faseCancelado
    case 'expired': return textos.faseExpirado
    case 'failed': return failureMessage(op.errorCode, idioma)
  }
}

/** As quatro etapas, com a atual em andamento e onde parou se não deu certo. */
function Etapas({ session }: { session: SetupSession }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const phase = session.operation?.phase ?? null
  const installing = session.step === 'install'
  const ended = phase === 'failed' || phase === 'expired' || phase === 'cancelled'
  const current = session.starting ? 0 : !phase || ended ? null : installing ? 0 : stepOf(phase)
  const stopped = ended ? Math.max(session.reached, 0) : null
  const connected = phase === 'connected' && !installing
  return (
    <ol className="contas-etapas" aria-label={tradutor(idioma)('contas.etapasRotulo')}>
      {textos.etapas.map((title, i) => {
        const done = connected || (current !== null && i < current) || (stopped !== null && i < stopped)
        const active = !connected && stopped === null && current === i
        const halted = stopped === i
        return (
          <li key={title} className="contas-etapa" aria-current={active ? 'step' : undefined}>
            <span aria-hidden className={`agent-step-number${done ? ' done' : ''}`}>
              {done ? <Icon name="checkmark" size={10} weight="bold" color="var(--on-accent)" />
                : active ? <ThinkingOrb state="connecting" size={14} color="var(--context-ink-2, var(--ink-2))" />
                : halted ? <Icon name="exclamationmark" size={11} weight="bold" color="var(--attention)" />
                : <Text size="caption" weight="semibold" tone="secondary">{i + 1}</Text>}
            </span>
            {/* O título visível e, para o leitor de tela, a frase inteira com o estado da etapa. */}
            <Text tone={active || done ? 'text' : 'secondary'} weight={active ? 'semibold' : undefined} aria-hidden={done || active || halted || undefined}>{title}</Text>
            {done || active || halted ? <span className="contas-sr-only">{tradutor(idioma)(done ? 'contas.etapaConcluida' : active ? 'contas.etapaEmAndamento' : 'contas.etapaParou', { etapa: title })}</span> : null}
          </li>
        )
      })}
    </ol>
  )
}

/** URL e código só aqui: nada vai para chat, histórico, registro ou disco. */
function Desafio({ session, operation, engine }: { session: SetupSession; operation: SetupOperation; engine: ManagedAccount['backend'] }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const challenge = operation.challenge!
  const engineUrl = providerUrl(engine, challenge.url, challenge.displayDomain)
  const [code, setCode] = useState('')
  const [copied, setCopied] = useState(false)
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => clearTimeout(copiedTimer.current), [])
  const overdue = session.overdue
  const expires = operation.expiresAt ? expiresAtMs(operation.expiresAt) : null
  const copy = () => {
    if (!challenge.code) return
    void navigator.clipboard?.writeText(challenge.code).then(() => {
      setCopied(true)
      clearTimeout(copiedTimer.current)
      copiedTimer.current = setTimeout(() => setCopied(false), 2500)
    }, () => {})
  }
  const submit = (e: FormEvent) => {
    e.preventDefault()
    const value = code
    setCode('')
    void session.submitCode(value)
  }
  return (
    <VStack gap={12} className="contas-desafio">
      {engineUrl ? (
        <HStack gap={6}>
          <Icon name="lock.shield" size={13} color="var(--context-ink-2, var(--ink-2))" />
          <Text size="subheadline" tone="secondary">{textos.paginaOficial(challenge.displayDomain)}</Text>
        </HStack>
      ) : (
        <Text tone="attention" role="alert">{textos.enderecoRecusado}</Text>
      )}
      {engineUrl && challenge.kind === 'device_code' && challenge.code ? (
        <VStack gap={6}>
          <Text tone="text">{textos.digiteCodigo}</Text>
          <HStack gap={8}>
            <Text mono tabular size="title2" weight="semibold" tone="text" className="contas-codigo selectable" dir="ltr" aria-label={tradutor(idioma)('contas.codigo', { codigo: challenge.code.split('').join(' ') })}>
              {challenge.code}
            </Text>
            <IconButton title={textos.copiarCodigo} icon="doc.on.doc" disabled={overdue} onClick={copy} />
            <span className="contas-sr-only" aria-live="polite">{copied ? textos.codigoCopiado : ''}</span>
          </HStack>
        </VStack>
      ) : null}
      {engineUrl ? (
        <HStack>
          <Button disabled={overdue} onClick={() => window.codync.app.openExternal(engineUrl)}>
            <HStack as="span" gap={6}>
              <span>{textos.abrirPagina}</span>
              <Icon name="arrow.up.right.square" size={11} />
            </HStack>
          </Button>
        </HStack>
      ) : null}
      {engineUrl && challenge.kind === 'paste_code' ? (
        <form className="contas-colar" onSubmit={submit}>
          <Text tone="text">{textos.colarCodigo}</Text>
          <HStack gap={8}>
            <div className="contas-campo">
              <Field value={code} onChange={setCode} secret mono label={textos.codigoDaPagina} placeholder={textos.codigoDaPagina} />
            </div>
            <Button kind="secondary" disabled={!code.trim() || session.submitting || overdue}>{session.submitting ? textos.enviando : textos.enviarCodigo}</Button>
          </HStack>
        </form>
      ) : null}
      {expires ? (
        <HStack gap={6}>
          <Icon name="clock" size={11} color={overdue ? 'var(--attention)' : 'var(--context-ink-2, var(--ink-2))'} />
          <Text size="caption" tone={overdue ? 'attention' : 'secondary'}>{overdue ? textos.venceu : textos.valeAte(clockText(expires, idioma))}</Text>
        </HStack>
      ) : null}
    </VStack>
  )
}

function Conectada({ account }: { account: ManagedAccount }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const view = accountView(account, 'cli', idioma)
  return (
    <VStack gap={6} className="contas-conectada fade-in">
      <HStack gap={8}>
        <Icon name="checkmark.circle.fill" size={14} color="var(--success)" />
        <Text tone="text" weight="semibold">{account.state === 'connected' ? `${view.state} · ${view.billing}` : view.state}</Text>
      </HStack>
      {account.state === 'connected' ? null : <Text size="caption" tone="secondary">{view.detail}</Text>}
      <Text size="caption" tone="secondary">{textos.escolhaNoBot}</Text>
      {account.backend === 'grok' ? <Text size="caption" tone="secondary">{tradutor(idioma)('contas.grokAviso')}</Text> : null}
    </VStack>
  )
}

function Rodape({ session, reconnect, forBot, onClose, accountId }: {
  session: SetupSession
  reconnect: boolean
  forBot?: { name: string; use: (accountId: string) => void }
  onClose: () => void
  accountId: string
}) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const op = session.operation
  const phase = op?.phase ?? null
  const active = session.active
  const terminal = session.canOpenTerminal
  let primary: { label: string; run: () => void; disabled?: boolean } | null = null
  if (!op && !session.starting) {
    primary = session.errorKind === 'api_form_required' ? null
      : session.error && session.step
      ? { label: textos.tentarDeNovo, run: () => session.retryStart() }
      : { label: reconnect ? textos.reconectar : textos.comecar, run: () => session.begin() }
  } else if (active && session.error) {
    primary = { label: textos.tentarDeNovo, run: () => session.retryReading() }
  } else if (phase === 'connected' && session.step !== 'install') {
    primary = forBot
      ? { label: tradutor(idioma)('contas.usarEm', { nome: forBot.name }), run: () => { forBot.use(accountId); onClose() } }
      : { label: textos.concluir, run: onClose }
  } else if (phase === 'expired' || (phase === 'awaiting_authorization' && session.overdue)) {
    primary = { label: textos.gerarNovoCodigo, run: () => session.restart(), disabled: phase !== 'expired' }
  } else if (phase === 'cancelled') {
    primary = { label: textos.comecarDeNovo, run: () => session.restart() }
  } else if (phase === 'failed') {
    primary = { label: textos.tentarDeNovo, run: () => session.restart() }
  }
  return (
    <HStack gap={8} className="contas-footer">
      {terminal ? (
        <button className="plain-link small" onClick={() => void session.openTerminal()}>
          <HStack as="span" gap={5}>
            <Icon name="terminal" size={11} />
            <span>{textos.abrirTerminal}</span>
          </HStack>
        </button>
      ) : null}
      <Spacer />
      {active ? (
        <Button kind="secondary" disabled={session.cancelling} onClick={() => void session.cancel()}>
          {session.cancelling ? textos.faseCancelando : textos.cancelar}
        </Button>
      ) : null}
      {phase === 'connected' && session.step !== 'install' && forBot ? <Button kind="secondary" onClick={onClose}>{textos.concluir}</Button> : null}
      {/* Fechar sempre à mão, menos quando Concluir já fecha. */}
      {phase === 'connected' && session.step !== 'install' ? null : <Button kind="secondary" onClick={onClose}>{textos.fechar}</Button>}
      {primary ? <Button disabled={primary.disabled} onClick={primary.run}>{primary.label}</Button> : null}
    </HStack>
  )
}
