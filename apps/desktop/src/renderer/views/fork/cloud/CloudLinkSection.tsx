import { useId, useState, type ReactNode } from 'react'
import type { ForkBootstrapPhase, ForkBootstrapState, ForkCloudComputer } from '@shared/fork-cloud'
import type { SSHAttachment } from '@shared/ssh'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useAccount } from '../../../store/account'
import { useApp } from '../../../store/context'
import { ComputerBadge } from '../../ComputerBadge'
import { Reveal, SectionHeader, spacedCode } from '../../settings/parts'
import { useSSH } from '../../settings/ssh-model'
import { isolado } from '../../../../shared/bidi.ts'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { aparelhoDe } from '../account/exit-copy'
import { CONNECTION_KINDS, connectionLine, deadlineText, statusLine, type Line } from './cloud-copy'
import type { ForkCloudModel, Notice } from './fork-cloud-model'
import { useForkCloud } from './use-fork-cloud'
import './cloud-link.css'

// DON-917 (fatia 9b): o aparelho escolhe a frase inteira do catálogo (`….mac`/`….pc`); o idioma vem de cada view.
const aparelho = aparelhoDe(window.codync.platform)
/**
 * O nome do Hub para desenhar: o real, ou o provisório do catálogo resolvido agora (R1-01 do #596). O modelo só guarda
 * o nome real ou null; `connect` nunca recebe este texto.
 */
const hubName = (cloud: ForkCloudModel, computerId: string | undefined, t: Tradutor) =>
  cloud.nameOf(computerId) ?? t(computerId && computerId === cloud.bootstrapHub ? 'nuvem.seuHub' : 'nuvem.oHub')
const avisoChave = (n: Notice) => (n === 'notSent' ? (`nuvem.aviso.notSent.${aparelho}` as const) : (`nuvem.aviso.${n}` as const))

/**
 * Ajustes › Computadores com `cloudAccess`: registrar este aparelho, achar os Hubs da conta, vincular
 * (pelo SSH já conectado, com o código conferido pelo dono ou como primeiro dono de um Hub novo, ACS4)
 * e conectar pela nuvem. Toda decisão é do processo principal; a tela mostra o estado dele e envia só
 * o que a pessoa clicou.
 */
export function CloudLinkSection() {
  const cloud = useForkCloud()
  const t = useT()
  if (!cloud) return null
  return (
    <div>
      <SectionHeader title={t('nuvem.secao')} />
      <CloudBody cloud={cloud} />
      <Reveal show={cloud.notice !== null}>
        <Text as="div" size="caption" tone="attention" role="status">{cloud.notice ? t(avisoChave(cloud.notice)) : null}</Text>
      </Reveal>
    </div>
  )
}

function CloudBody({ cloud }: { cloud: ForkCloudModel }) {
  const account = useAccount()
  const idioma = useIdioma()
  const t = useT()
  const s = cloud.state
  if (!account.isConfigured) return <Note>{t('nuvem.semContaConfigurada')}</Note>
  if (!account.isSignedIn) return <Note>{t(`nuvem.entre.${aparelho}`)}</Note>
  if (cloud.unreachable) {
    return (
      <Card>
        <HStack gap={10}>
          <Text size="caption" tone="secondary" grow role="status">{t('nuvem.naoRespondeu')}</Text>
          <Button kind="secondary" onClick={() => cloud.refresh()}>{t('nuvem.conferirDeNovo')}</Button>
        </HStack>
      </Card>
    )
  }
  if (!s) return <Note>{t('nuvem.conferindoAcesso')}</Note>
  if (s.kind === 'unknown') return <UncertainCard cloud={cloud} />
  if (cloud.registration === 'unavailable') return <Note>{t('nuvem.indisponivelVersao')}</Note>
  if (cloud.registration === 'unregistered') return <RegisterCard cloud={cloud} />
  // ACS4: enquanto a primeira vinculação bloqueia, só ela aparece; o Main também recusa o resto.
  // Um pedido aberto ou encerrado que o Main liberou (`blocksOtherOperations: false`) fica entre os cartões ociosos.
  if (cloud.bootstrapBlocks && cloud.bootstrap) {
    return cloud.bootstrapClosed
      ? <BootstrapClosedCard cloud={cloud} bootstrap={cloud.bootstrap} />
      : <BootstrapCard cloud={cloud} bootstrap={cloud.bootstrap} />
  }
  const firstLinked = cloud.bootstrapLinked
  if (firstLinked) return <BootstrapLinkedCard cloud={cloud} computerId={firstLinked} />
  if (s.kind === 'linking' || s.kind === 'waitingOwnerApproval' || s.kind === 'linked') return <EnrollmentCard cloud={cloud} />
  if (CONNECTION_KINDS.includes(s.kind) || cloud.flow?.kind === 'connect') return <ConnectionCard cloud={cloud} />
  const ended = cloud.flow ? statusLine(s.kind, cloud.flow.name ?? hubName(cloud, cloud.flow.computerId, t), cloud.flow.kind, aparelho, idioma) : null
  if (ended) return <FlowEndCard cloud={cloud} line={ended} />
  return <IdleCards cloud={cloud} />
}

function Note({ children }: { children: ReactNode }) {
  return <Text as="div" size="callout" tone="secondary" role="status">{children}</Text>
}

function Card({ children, busy = false }: { children: ReactNode; busy?: boolean }) {
  return <div className="settings-card settings-appear" aria-busy={busy || undefined}>{children}</div>
}

function StatusText({ line }: { line: Line }) {
  return <Text as="div" size="caption" tone={line.tone} role="status">{line.text}</Text>
}

/** O título de um cartão com a nota sob ele, e a ação à direita. */
function CardHeader({ title, detail, children }: { title: ReactNode; detail?: ReactNode; children?: ReactNode }) {
  return (
    <HStack gap={10}>
      <VStack grow className="settings-stack" gap={2}>
        <Text size="callout" weight="medium">{title}</Text>
        {detail ? <Text size="caption" tone="secondary">{detail}</Text> : null}
      </VStack>
      {children}
    </HStack>
  )
}

const hubBadge = (id: string, name: string) => ({ id, name, signKey: '', urls: [] })

// MARK: registro

function RegisterCard({ cloud }: { cloud: ForkCloudModel }) {
  const idioma = useIdioma()
  const t = useT()
  const s = cloud.state
  const status = s ? statusLine(s.kind, t('nuvem.oHub'), null, aparelho, idioma) : null
  const busy = cloud.busy !== null
  return (
    <Card busy={cloud.busy === 'registerDevice' || cloud.busy === 'reconcileRegistration'}>
      <CardHeader title={t(`nuvem.registrar.titulo.${aparelho}`)} detail={t(`nuvem.registrar.detalhe.${aparelho}`)}>
        <Button onClick={() => void cloud.registerDevice()} disabled={busy}>
          {cloud.busy === 'registerDevice' ? t('nuvem.registrar.registrando') : t('nuvem.registrar.botao')}
        </Button>
      </CardHeader>
      {status ? <StatusText line={status} /> : null}
      {/* Depois de reabrir o app, um registro feito antes pode só faltar ser conferido. */}
      <HStack gap={10}>
        <Text size="caption" tone="secondary" grow>{t(`nuvem.registrar.jaRegistrou.${aparelho}`)}</Text>
        <Button kind="secondary" onClick={() => void cloud.reconcileRegistration()} disabled={busy}>
          {cloud.busy === 'reconcileRegistration' ? t('nuvem.verificando') : t('nuvem.verificarRegistro')}
        </Button>
      </HStack>
    </Card>
  )
}

// MARK: sem nada em andamento

function IdleCards({ cloud }: { cloud: ForkCloudModel }) {
  const ssh = useSSH()
  const idioma = useIdioma()
  const t = useT()
  const s = cloud.state
  const status = s ? statusLine(s.kind, hubName(cloud, s.computerId, t), null, aparelho, idioma) : null
  const local = ssh.state.attachments.filter((a) => ssh.status(a.profileId).kind === 'connected')
  return (
    <VStack gap={12}>
      <HubsCard cloud={cloud} status={status} />
      {local.length > 0 ? <LocalCard cloud={cloud} attachments={local} /> : null}
      {cloud.bootstrap?.phase === 'available' ? <BootstrapOfferCard cloud={cloud} bootstrap={cloud.bootstrap} /> : null}
      {cloud.bootstrap && cloud.bootstrapOpen && !cloud.bootstrapBlocks ? <BootstrapCard cloud={cloud} bootstrap={cloud.bootstrap} /> : null}
      {cloud.bootstrap && cloud.bootstrapClosed && !cloud.bootstrapBlocks ? <BootstrapClosedCard cloud={cloud} bootstrap={cloud.bootstrap} /> : null}
    </VStack>
  )
}

/** Os Hubs que o Main achou na conta nesta sessão: conectar (já vinculado) ou pedir acesso (A2). */
function HubsCard({ cloud, status }: { cloud: ForkCloudModel; status: Line | null }) {
  const t = useT()
  const discovering = cloud.busy === 'discover' || cloud.state?.kind === 'discovering'
  // O Hub da primeira vinculação concluída, até a procura trazê-lo com o nome dele.
  const first = cloud.bootstrapHub
  const firstUnlisted = first && !cloud.computers.some((c) => c.computerId === first) ? first : null
  return (
    <Card busy={discovering}>
      <CardHeader title={t('nuvem.hubs.titulo')} detail={t(`nuvem.hubs.detalhe.${aparelho}`)}>
        <Button kind="secondary" onClick={() => void cloud.discover()} disabled={cloud.busy !== null}>
          {discovering ? t('nuvem.hubs.procurando') : t('nuvem.hubs.procurar')}
        </Button>
      </CardHeader>
      {status && !discovering && !(firstUnlisted && cloud.state?.kind === 'noHub') ? <StatusText line={status} /> : null}
      {firstUnlisted ? <FirstHubRow cloud={cloud} computerId={firstUnlisted} /> : null}
      {cloud.computers.map((computer) => <HubRow key={computer.computerId} cloud={cloud} computer={computer} />)}
    </Card>
  )
}

function HubRow({ cloud, computer }: { cloud: ForkCloudModel; computer: ForkCloudComputer }) {
  const app = useApp()
  // Com o SSH (ou o host deste computador) já mostrando esse Hub, a conexão direta tem prioridade.
  const direct = app.store(computer.computerId)?.isLoopback === true
  const t = useT()
  const busy = cloud.busy !== null
  return (
    <div className="settings-row settings-appear">
      <ComputerBadge computer={hubBadge(computer.computerId, computer.name)} size={22} />
      <VStack grow className="settings-stack">
        <Text size="callout" weight="medium" truncate><bdi>{computer.name}</bdi></Text>
        <Text size="caption" tone="secondary">{direct ? t(`nuvem.hubs.direto.${aparelho}`) : t('nuvem.hubs.naConta')}</Text>
      </VStack>
      <Button kind="secondary" onClick={() => void cloud.startRemote(computer)} disabled={busy}>{t('nuvem.hubs.pedirAcesso')}</Button>
      <Button onClick={() => void cloud.connect(computer.computerId, computer.name)} disabled={busy}>{t('nuvem.conectar')}</Button>
    </div>
  )
}

/** A1: um Hub conectado por SSH confere o código direto nele e aprova este aparelho. */
function LocalCard({ cloud, attachments }: { cloud: ForkCloudModel; attachments: SSHAttachment[] }) {
  const t = useT()
  return (
    <Card busy={cloud.busy === 'startLocal'}>
      <CardHeader title={t('nuvem.ssh.titulo')} detail={t(`nuvem.ssh.detalhe.${aparelho}`)} />
      {attachments.map((a) => <LocalRow key={a.profileId} cloud={cloud} attachment={a} />)}
    </Card>
  )
}

function LocalRow({ cloud, attachment }: { cloud: ForkCloudModel; attachment: SSHAttachment }) {
  const app = useApp()
  const account = useAccount()
  const idioma = useIdioma()
  const t = useT()
  const store = app.store(attachment.computer.id)
  useModel(store)
  const [confirmClaim, setConfirmClaim] = useState(false)
  const name = store?.hostName ?? attachment.computer.name
  // Só apresentação: o Main confere de novo a nuvem e o dono pelo mesmo SSH antes de qualquer efeito.
  const status = store?.isLoopback ? store.cloud : null
  const owner = status?.owner
  const busy = cloud.busy !== null
  const start = (claim: boolean) => void cloud.startLocal(attachment.profileId, attachment.computer.id, name, claim)

  let detail: string
  let action: ReactNode = null
  if (status && !status.enabled) {
    detail = t('nuvem.ssh.nuvemDesligada')
  } else if (status && owner === null) {
    detail = t('nuvem.ssh.semConta')
    action = <Button onClick={() => setConfirmClaim(true)} disabled={busy}>{t('nuvem.ssh.adicionar')}</Button>
  } else if (owner && owner.userId !== account.user?.userId) {
    detail = t(`nuvem.ssh.outraConta.${aparelho}`)
  } else {
    detail = owner ? t('nuvem.hubs.naConta') : t('nuvem.ssh.conectadoSsh')
    action = <Button onClick={() => start(false)} disabled={busy}>{cloud.busy === 'startLocal' ? t('nuvem.ssh.vinculando') : t(`nuvem.ssh.vincular.${aparelho}`)}</Button>
  }

  return (
    <div className="settings-row settings-appear">
      <ComputerBadge computer={store?.computer ?? hubBadge(attachment.computer.id, name)} size={22} />
      <VStack grow className="settings-stack">
        <Text size="callout" weight="medium" truncate><bdi>{name}</bdi></Text>
        <Text size="caption" tone="secondary">{detail}</Text>
      </VStack>
      {action}
      <Dialog
        open={confirmClaim}
        title={t('nuvem.ssh.adicionarTitulo', { nome: isolado(name) })}
        message={t(`nuvem.ssh.adicionarMensagem.${aparelho}`, { nome: isolado(name) })}
        actions={[{ title: t('nuvem.ssh.adicionarEVincular'), action: () => start(true) }]}
        cancel={t('conta.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmClaim(false)}
      />
    </div>
  )
}

// MARK: vínculo (A1 e A2)

function EnrollmentCard({ cloud }: { cloud: ForkCloudModel }) {
  const idioma = useIdioma()
  const t = useT()
  const s = cloud.state!
  const name = hubName(cloud, s.computerId, t)
  const nome = isolado(name)
  const local = cloud.flow?.kind === 'local'
  const attempt = cloud.currentAttempt
  const busy = cloud.busy !== null
  const until = s.kind === 'linked' ? null : deadlineText(s.deadline, idioma)

  let body: ReactNode
  if (s.kind === 'linked') {
    body = (
      <>
        <StatusText line={{ text: t(`nuvem.vinculo.gravado.${aparelho}`, { nome }), tone: 'secondary' }} />
        <HStack>
          <Button onClick={() => s.computerId && void cloud.connect(s.computerId, cloud.nameOf(s.computerId))} disabled={busy || !s.computerId}>
            {cloud.busy === 'connect' ? t('nuvem.conectando') : t('nuvem.conectar')}
          </Button>
        </HStack>
      </>
    )
  } else if (s.kind === 'waitingOwnerApproval' && attempt && s.code) {
    body = local
      ? <LocalCodeStep cloud={cloud} code={s.code} name={name} />
      : <CompareStep key={attempt} cloud={cloud} attemptId={attempt} code={s.code} name={name} secondCode={s.secondCode === true} />
  } else if (!attempt) {
    body = <StatusText line={{ text: local ? t('nuvem.vinculo.conferindoSsh', { nome }) : t('nuvem.vinculo.preparando'), tone: 'secondary' }} />
  } else if (cloud.revealed === attempt) {
    body = <CheckStep cloud={cloud} text={local ? t(`nuvem.vinculo.aprovadoSsh.${aparelho}`) : t('nuvem.vinculo.continueGravar')} label={t('nuvem.vinculo.gravar')} />
  } else {
    body = <CheckStep cloud={cloud} text={local ? t('nuvem.vinculo.pedidoCriado', { nome }) : t('nuvem.vinculo.pedidoEnviado', { nome })} label={local ? t('nuvem.continuar') : t('nuvem.verificar')} />
  }

  return (
    <Card busy={busy}>
      <HStack gap={10}>
        <Text size="callout" weight="medium" grow truncate>{local ? t('nuvem.vinculo.tituloSsh', { nome }) : t('nuvem.vinculo.tituloAcesso', { nome })}</Text>
        {until ? <Text size="caption" tone="tertiary">{until}</Text> : null}
      </HStack>
      {s.readUnavailable ? <StatusText line={{ text: t('nuvem.vinculo.consultaSemResposta'), tone: 'attention' }} /> : null}
      {body}
      {s.kind === 'linked' ? null : attempt ? (
        <HStack>
          <Button kind="secondary" onClick={() => void cloud.cancel()} disabled={cloud.busy === 'cancel' || cloud.busy === 'disconnect'}>{t('nuvem.vinculo.cancelarPedido')}</Button>
        </HStack>
      ) : (
        // Antes de o pedido existir só há o que este Mac começou: parar não muda nada no Hub.
        <HStack>
          <Button kind="secondary" onClick={() => void cloud.disconnect()} disabled={cloud.busy === 'disconnect'}>{t('nuvem.parar')}</Button>
        </HStack>
      )}
    </Card>
  )
}

/** Um passo que só avança quando a pessoa pede: busca o código, a aprovação ou a gravação, uma vez. */
function CheckStep({ cloud, text, label }: { cloud: ForkCloudModel; text: string; label: string }) {
  const t = useT()
  return (
    <>
      <StatusText line={{ text, tone: 'secondary' }} />
      <HStack>
        <Button onClick={() => void cloud.check()} disabled={cloud.busy !== null}>{cloud.busy === 'check' ? t('nuvem.verificando') : label}</Button>
      </HStack>
    </>
  )
}

/** O código deste pedido, para comparar; não se seleciona nem se copia. */
function CodeDisplay({ code }: { code: string }) {
  const t = useT()
  return (
    <VStack align="center" gap={2} className="cloud-link-code">
      <Text size="caption" tone="secondary">{t(`nuvem.codigo.rotulo.${aparelho}`)}</Text>
      <Text as="div" size={34} weight="semibold" mono tone="text" dir="ltr" aria-label={t('nuvem.codigo.aria', { codigo: code.split('').join(' ') })}>{spacedCode(code)}</Text>
    </VStack>
  )
}

/** A1: o Main confere este código no Hub pelo SSH e aprova o próprio pedido; nada é digitado. */
function LocalCodeStep({ cloud, code, name }: { cloud: ForkCloudModel; code: string; name: string }) {
  const t = useT()
  return (
    <>
      <CodeDisplay code={code} />
      <StatusText line={{ text: t(`nuvem.codigo.sshAoContinuar.${aparelho}`, { nome: isolado(name) }), tone: 'secondary' }} />
      <HStack>
        <Button onClick={() => void cloud.check()} disabled={cloud.busy !== null}>{cloud.busy === 'check' ? t('nuvem.codigo.conferindo') : t('nuvem.codigo.conferirSsh')}</Button>
      </HStack>
    </>
  )
}

/**
 * A2: a pessoa segue a cerimônia indicada e digita o código vindo do aparelho do dono.
 * O campo começa vazio, nunca é preenchido daqui e só é enviado pelo botão; volta vazio em
 * qualquer resposta que não seja aceita (e num pedido novo, pela `key`).
 */
function CompareStep({ cloud, attemptId, code, name, secondCode }: { cloud: ForkCloudModel; attemptId: string; code: string; name: string; secondCode: boolean }) {
  const t = useT()
  const [typed, setTyped] = useState('')
  const field = useId()
  const hint = useId()
  const busy = cloud.busy !== null

  if (cloud.compared === attemptId) {
    return <CheckStep cloud={cloud} text={t(`nuvem.codigo.conferido.${aparelho}`)} label={t('nuvem.codigo.verificarAprovacao')} />
  }

  const send = async () => {
    const outcome = await cloud.confirm(typed)
    if (outcome !== 'accepted') setTyped('')
  }

  return (
    <>
      <CodeDisplay code={code} />
      <Text as="div" size="callout" tone="secondary" id={hint}>
        {t(secondCode ? 'nuvem.codigo.instrucaoSegundo' : 'nuvem.codigo.instrucao', { nome: isolado(name) })}
      </Text>
      <HStack gap={8} wrap>
        <label htmlFor={field}><Text size="callout">{t('nuvem.codigo.noDono')}</Text></label>
        <Spacer />
        <input
          id={field}
          className="cloud-link-digits"
          aria-describedby={hint}
          value={typed}
          dir="ltr"
          placeholder={t('nuvem.codigo.seisDigitos')}
          inputMode="numeric"
          autoComplete="off"
          spellCheck={false}
          maxLength={7}
          autoFocus
          disabled={busy}
          onChange={(e) => setTyped(e.target.value.replace(/\D/g, '').slice(0, 6))}
        />
        <Button onClick={() => void send()} disabled={busy || typed.length !== 6}>
          {cloud.busy === 'confirm' ? t('nuvem.codigo.confirmando') : t('nuvem.codigo.confirmar')}
        </Button>
      </HStack>
      {!secondCode && <Text as="div" size="caption" tone="tertiary">{t('nuvem.codigo.diferentes')}</Text>}
    </>
  )
}

/** O pedido ou o vínculo que a pessoa começou terminou sem passo seguinte. */
function FlowEndCard({ cloud, line }: { cloud: ForkCloudModel; line: Line }) {
  const t = useT()
  const s = cloud.state!
  const flow = cloud.flow!
  const mismatch = s.kind === 'hubUnverified' && !!s.attemptId && cloud.compared === s.attemptId
  const shown: Line = mismatch ? { text: t(`nuvem.codigo.naoConfere.${aparelho}`), tone: 'danger' } : line
  return (
    <Card>
      <CardHeader title={t(flow.kind === 'local' ? 'nuvem.vinculo.tituloSsh' : 'nuvem.vinculo.tituloAcesso', { nome: isolado(flow.name ?? hubName(cloud, flow.computerId, t)) })}>
        <Button kind="secondary" onClick={() => void cloud.disconnect()} disabled={cloud.busy === 'disconnect'}>{t('nuvem.recomecar')}</Button>
      </CardHeader>
      <StatusText line={shown} />
    </Card>
  )
}

// MARK: primeira vinculação (ACS4)

/** Nome neutro do Hub da primeira vinculação (`nuvem.seuHub`), até a procura ou a conexão trazerem o dele. */

type BootstrapIntent = 'startBootstrap' | 'checkBootstrap' | 'proveBootstrap' | 'reconcileBootstrap'
const BOOTSTRAP_KINDS: readonly string[] = ['startBootstrap', 'checkBootstrap', 'proveBootstrap', 'reconcileBootstrap']

/** O que uma fase diz e a única ação dela; o botão ainda depende do `can*` que o Main publicou. */
interface BootstrapStep {
  readonly step: 1 | 2 | 3 | 4
  readonly line: Line
  readonly hint?: string
  /** O prazo do pedido ainda importa para o passo. */
  readonly deadline?: true
  /** O prazo já passou, mas o retrato ainda bloqueia: só reler o estado do Main, sem rede nem nova tentativa. */
  readonly reread?: true
  readonly action?: { readonly kind: BootstrapIntent; readonly label: string; readonly busy: string }
}

/**
 * Abertura do canal e desafio: a prova só sai depois que o Main grava `proofUnknown`, então nestas
 * fases nenhuma prova foi enviada. Elas não se repetem; vencido o prazo, o Main pode liberar o resto.
 */
const BEFORE_PROOF: readonly ForkBootstrapPhase[] = ['openingUnknown', 'challengeUnknown', 'challenged']

/**
 * O passo de cada fase publicada pelo Main. Só descreve: incerteza permite apenas consultar a mesma
 * tentativa ou, na confirmação, pedir pelo clique a mesma publicação idempotente do mesmo recibo;
 * nada aqui repete pedido, autorização, abertura ou prova. O relógio da tela só muda o texto e oferece
 * reler o Main; quem libera é o `blocksOtherOperations: false` dele. `published` ainda não é vínculo:
 * o Main só grava depois do recibo de ativação do Hub.
 */
function bootstrapStep(b: ForkBootstrapState, now: number, t: Tradutor): BootstrapStep {
  const CHECK = { kind: 'checkBootstrap', label: t('nuvem.boot.verificarPedido'), busy: t('nuvem.verificando') } as const
  const lapsed = b.blocksOtherOperations === false || (typeof b.deadline === 'number' && now >= b.deadline)
  // Vencido antes de qualquer prova: o Main libera as outras ações, mas esta geração não recomeça.
  const expired = (step: 2 | 3): BootstrapStep => ({
    step,
    line: { text: t('nuvem.boot.expirado'), tone: 'attention' },
    hint: t('nuvem.boot.informarSuporte'),
    ...(b.blocksOtherOperations === false ? {} : { reread: true as const }),
  })
  if (BEFORE_PROOF.includes(b.phase)) {
    if (lapsed) return expired(3)
    return {
      step: 3,
      line: { text: t(`nuvem.boot.parouAntes.${aparelho}`), tone: 'attention' },
      hint: t('nuvem.boot.quandoPrazo'),
      deadline: true,
    }
  }
  switch (b.phase) {
    case 'prepared':
      return {
        step: 1,
        line: { text: t(`nuvem.boot.semEfeito.${aparelho}`), tone: 'secondary' },
        action: { kind: 'startBootstrap', label: t('nuvem.boot.enviarPedido'), busy: t('nuvem.boot.enviando') },
      }
    case 'createUnknown':
      return {
        step: 1,
        line: { text: t('nuvem.boot.envioSemResposta'), tone: 'attention' },
        action: CHECK,
      }
    case 'requested':
      if (b.canProve) {
        return {
          step: 3,
          line: { text: t(`nuvem.boot.autorizacaoChegou.${aparelho}`), tone: 'secondary' },
          hint: t('nuvem.boot.mantenhaAberto'),
          deadline: true,
          action: { kind: 'proveBootstrap', label: t(`nuvem.boot.vincular.${aparelho}`), busy: t('nuvem.boot.vinculando') },
        }
      }
      if (lapsed) return expired(2)
      return {
        step: 2,
        line: { text: t(`nuvem.boot.registrado.${aparelho}`), tone: 'secondary' },
        deadline: true,
        action: { kind: 'checkBootstrap', label: t('nuvem.boot.verificarAutorizacao'), busy: t('nuvem.verificando') },
      }
    case 'proofUnknown':
      // A prova pode ter saído: continua bloqueado até a mesma tentativa ser conferida.
      return {
        step: 3,
        line: { text: t(`nuvem.boot.provaIncerta.${aparelho}`), tone: 'attention' },
        hint: t(`nuvem.boot.provaNaoAparece.${aparelho}`),
        action: CHECK,
      }
    case 'receipt':
      return {
        step: 4,
        line: { text: t(`nuvem.boot.hubRecebeu.${aparelho}`), tone: 'secondary' },
        action: { kind: 'reconcileBootstrap', label: t('nuvem.boot.confirmarVinculo'), busy: t('nuvem.boot.confirmando') },
      }
    case 'reconcileUnknown':
      // Com `canReconcile`, o Main lê o mesmo pedido e só pede a mesma publicação se ela ainda faltar.
      if (b.canReconcile) {
        return {
          step: 4,
          line: { text: t(`nuvem.boot.confirmacaoSemResposta.${aparelho}`), tone: 'attention' },
          action: { kind: 'reconcileBootstrap', label: t('nuvem.boot.verificarConcluir'), busy: t('nuvem.verificando') },
        }
      }
      return {
        step: 4,
        line: { text: t('nuvem.boot.confirmacaoVerifique'), tone: 'attention' },
        action: CHECK,
      }
    case 'published':
      return {
        step: 4,
        line: { text: t(`nuvem.boot.publicado.${aparelho}`), tone: 'secondary' },
        action: { kind: 'reconcileBootstrap', label: t('nuvem.boot.conferirAtivacao'), busy: t('nuvem.boot.conferindo') },
      }
    case 'pinUnknown':
      return {
        step: 4,
        line: { text: t(`nuvem.boot.gravacaoSemResposta.${aparelho}`), tone: 'attention' },
        action: { kind: 'reconcileBootstrap', label: t('nuvem.boot.concluirGravacao'), busy: t('nuvem.boot.gravando') },
      }
    default:
      // `available`, `complete` e `closed` têm cartões próprios; aqui nada é oferecido.
      return { step: 1, line: { text: t('nuvem.boot.conferindoPrimeira'), tone: 'secondary' } }
  }
}

const offered = (b: ForkBootstrapState, kind: BootstrapIntent) =>
  kind === 'startBootstrap' ? b.canStart : kind === 'checkBootstrap' ? b.canCheck : kind === 'proveBootstrap' ? b.canProve : b.canReconcile

function sendBootstrap(cloud: ForkCloudModel, kind: BootstrapIntent) {
  switch (kind) {
    case 'startBootstrap': return cloud.startBootstrap()
    case 'checkBootstrap': return cloud.checkBootstrap()
    case 'proveBootstrap': return cloud.proveBootstrap()
    case 'reconcileBootstrap': return cloud.reconcileBootstrap()
  }
}

/** ACS4: um Hub que a nuvem preparou para a conta e ainda não tem dono. Só sai pelo clique, depois de confirmar. */
function BootstrapOfferCard({ cloud, bootstrap }: { cloud: ForkCloudModel; bootstrap: ForkBootstrapState }) {
  const [confirm, setConfirm] = useState(false)
  const idioma = useIdioma()
  const t = useT()
  return (
    <Card busy={cloud.busy === 'startBootstrap'}>
      <CardHeader title={t('nuvem.boot.oferta.titulo')} detail={t(`nuvem.boot.oferta.detalhe.${aparelho}`)}>
        <Button kind="secondary" onClick={() => setConfirm(true)} disabled={cloud.busy !== null || !bootstrap.canStart}>
          {cloud.busy === 'startBootstrap' ? t('nuvem.boot.enviando') : t('nuvem.boot.oferta.pedir')}
        </Button>
      </CardHeader>
      <Dialog
        open={confirm}
        title={t('nuvem.boot.oferta.confirmarTitulo')}
        message={t(`nuvem.boot.oferta.confirmarMensagem.${aparelho}`)}
        actions={[{ title: t('nuvem.boot.enviarPedido'), action: () => void cloud.startBootstrap() }]}
        cancel={t('conta.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirm(false)}
      />
    </Card>
  )
}

/**
 * Um pedido de primeira vinculação aberto: a fase do Main, a ação dela (se houver) e o identificador.
 * Sozinho na seção enquanto bloqueia; entre os cartões ociosos quando o Main o libera explicitamente.
 */
function BootstrapCard({ cloud, bootstrap: b }: { cloud: ForkCloudModel; bootstrap: ForkBootstrapState }) {
  const idioma = useIdioma()
  const t = useT()
  const step = bootstrapStep(b, Date.now(), t)
  const action = step.action
  const until = step.deadline ? deadlineText(b.deadline, idioma) : null
  const footnote = cloud.bootstrapBlocks
    ? t(`nuvem.boot.bloqueia.${aparelho}`)
    : b.phase === 'prepared'
      ? t('nuvem.boot.porEnquanto')
      : t('nuvem.boot.liberado')
  return (
    <Card busy={cloud.busy !== null && BOOTSTRAP_KINDS.includes(cloud.busy)}>
      <HStack gap={10}>
        <Text size="callout" weight="medium" grow truncate>{t(`nuvem.boot.titulo.${aparelho}`)}</Text>
        <Text size="caption" tone="tertiary">{t('nuvem.boot.etapa', { etapa: step.step })}</Text>
      </HStack>
      {b.readUnavailable && b.phase !== 'createUnknown' && b.phase !== 'prepared'
        ? <StatusText line={{ text: t('nuvem.boot.ultimaConsulta'), tone: 'attention' }} />
        : null}
      <StatusText line={step.line} />
      {until ? <Text as="div" size="caption" tone="tertiary">{until}</Text> : null}
      {action ? (
        <HStack>
          <Button onClick={() => void sendBootstrap(cloud, action.kind)} disabled={cloud.busy !== null || !offered(b, action.kind)}>
            {cloud.busy === action.kind ? action.busy : action.label}
          </Button>
        </HStack>
      ) : null}
      {step.reread ? (
        <HStack>
          <Button kind="secondary" onClick={() => cloud.refresh()} disabled={cloud.busy !== null}>{t('nuvem.conferirDeNovo')}</Button>
        </HStack>
      ) : null}
      {step.hint ? <Text as="div" size="caption" tone="secondary">{step.hint}</Text> : null}
      <VStack gap={2}>
        <Text size="caption" tone="tertiary">{footnote}</Text>
        <RequestId id={b.bootstrapId} />
      </VStack>
    </Card>
  )
}

/** O desfecho de um pedido encerrado, no lugar da etapa (`nuvem.boot.fechado*.<motivo>`); sem motivo conhecido, só encerrado. */

/**
 * ACS4 encerrado (cancelado ou vencido) sem virar vínculo: só histórico, com o identificador para o
 * suporte. Não oferece conectar, repetir, apagar nem pedir de novo. A prova pode ter saído antes do
 * encerramento, e o Main guarda o que já existia. Só o `false` explícito dele libera as outras ações;
 * até lá, o único botão relê o estado, sem rede nem nova tentativa.
 */
function BootstrapClosedCard({ cloud, bootstrap: b }: { cloud: ForkCloudModel; bootstrap: ForkBootstrapState }) {
  const t = useT()
  const held = cloud.bootstrapBlocks
  const outcome = b.terminalReason ?? 'closed'
  return (
    <Card>
      <HStack gap={10}>
        <Text size="callout" weight="medium" grow truncate>{t('nuvem.boot.fechadoTitulo')}</Text>
        <Text size="caption" tone="tertiary">{t(`nuvem.boot.fechado.${outcome}`)}</Text>
      </HStack>
      <StatusText line={{ text: t(`nuvem.boot.fechadoTexto.${outcome}`), tone: held ? 'attention' : 'secondary' }} />
      {held ? (
        <HStack>
          <Button kind="secondary" onClick={() => cloud.refresh()} disabled={cloud.busy !== null}>{t('nuvem.conferirDeNovo')}</Button>
        </HStack>
      ) : null}
      <Text as="div" size="caption" tone="secondary">{t('nuvem.boot.guardado')}</Text>
      <VStack gap={2}>
        <Text size="caption" tone="tertiary">{held ? t(`nuvem.boot.naoConfirmouEncerramento.${aparelho}`) : t('nuvem.boot.liberado')}</Text>
        <RequestId id={b.bootstrapId} />
      </VStack>
    </Card>
  )
}

/** O identificador do pedido, para conferir com o suporte: selecionável, sem botão de copiar nem código a comparar. */
function RequestId({ id }: { id: string }) {
  const t = useT()
  return (
    <Text size="caption" tone="tertiary">
      {t('nuvem.boot.identificador')} <Text mono dir="ltr" className="selectable">{id}</Text>
    </Text>
  )
}

/** ACS4 concluído: o Main gravou o vínculo e publicou `linked`; conectar continua sendo outro clique. */
function BootstrapLinkedCard({ cloud, computerId }: { cloud: ForkCloudModel; computerId: string }) {
  const t = useT()
  const name = hubName(cloud, computerId, t)
  return (
    <Card busy={cloud.busy === 'connect'}>
      <HStack gap={10}>
        <ComputerBadge computer={hubBadge(computerId, name)} size={22} />
        <VStack grow className="settings-stack">
          <Text size="callout" weight="medium" truncate><bdi>{name}</bdi></Text>
          <Text size="caption" tone="secondary" role="status">{t(`nuvem.boot.vinculoGravado.${aparelho}`)}</Text>
        </VStack>
        <Button onClick={() => void cloud.connect(computerId, cloud.nameOf(computerId))} disabled={cloud.busy !== null}>
          {cloud.busy === 'connect' ? t('nuvem.conectando') : t('nuvem.conectar')}
        </Button>
      </HStack>
    </Card>
  )
}

/** O Hub da primeira vinculação na lista, só com o identificador devolvido pelo Main e um nome neutro. */
function FirstHubRow({ cloud, computerId }: { cloud: ForkCloudModel; computerId: string }) {
  const app = useApp()
  const t = useT()
  const name = hubName(cloud, computerId, t)
  const direct = app.store(computerId)?.isLoopback === true
  return (
    <div className="settings-row settings-appear">
      <ComputerBadge computer={hubBadge(computerId, name)} size={22} />
      <VStack grow className="settings-stack">
        <Text size="callout" weight="medium" truncate><bdi>{name}</bdi></Text>
        <Text size="caption" tone="secondary">{direct ? t(`nuvem.hubs.direto.${aparelho}`) : t(`nuvem.boot.primeiroDono.${aparelho}`)}</Text>
      </VStack>
      <Button onClick={() => void cloud.connect(computerId, cloud.nameOf(computerId))} disabled={cloud.busy !== null}>{t('nuvem.conectar')}</Button>
    </div>
  )
}

// MARK: conexão

function ConnectionCard({ cloud }: { cloud: ForkCloudModel }) {
  const app = useApp()
  const idioma = useIdioma()
  const t = useT()
  const s = cloud.state!
  const name = hubName(cloud, s.computerId, t)
  const attached = cloud.readyAttachment
  const live = CONNECTION_KINDS.includes(s.kind)
  // O anexo da ponte cede o lugar ao SSH e ao host deste computador (use-fork-cloud).
  const direct = !!attached && app.store(attached.computerId)?.isLoopback === true
  const line: Line = live
    ? connectionLine(s.kind, name, !!attached, direct, idioma)
    : s.kind === 'error'
      ? { text: t(`nuvem.conexao.falhou.${aparelho}`, { nome: isolado(name) }), tone: 'attention' }
      : (statusLine(s.kind, name, 'connect', aparelho, idioma) ?? connectionLine('connecting', name, false, false, idioma))
  return (
    <Card busy={cloud.busy === 'connect'}>
      <HStack gap={10}>
        <ComputerBadge computer={hubBadge(s.computerId ?? '', name)} size={22} />
        <VStack grow className="settings-stack">
          <Text size="callout" weight="medium" truncate><bdi>{name}</bdi></Text>
          <Text size="caption" tone={line.tone} role="status">{line.text}</Text>
        </VStack>
        <Button kind="secondary" onClick={() => void cloud.disconnect()} disabled={cloud.busy === 'disconnect'}>{live ? t('nuvem.desconectar') : t('nuvem.recomecar')}</Button>
      </HStack>
    </Card>
  )
}

// MARK: resultado incerto

/** Nada é repetido: só verificar o registro, o pedido atual ou a gravação do vínculo, cada um por um clique. */
function UncertainCard({ cloud }: { cloud: ForkCloudModel }) {
  const t = useT()
  const s = cloud.state!
  const name = cloud.nameOf(s.computerId)
  const attempt = cloud.currentAttempt
  const publication = cloud.recoveryPhase === 'publication'
  const busy = cloud.busy !== null
  if (cloud.recovery === 'registration') {
    // O registro deste aparelho ficou sem resposta: só a conferência pelo Main, sem registrar de novo.
    return (
      <Card busy={cloud.busy === 'reconcileRegistration'}>
        <HStack gap={10} align="start">
          <Icon name="exclamationmark.triangle" size={14} color="var(--text-attention, var(--attention))" />
          <VStack grow className="settings-stack" gap={2}>
            <Text size="callout" weight="medium">{t('nuvem.incerto.registroTitulo')}</Text>
            <Text size="caption" tone="secondary" role="status">{t(`nuvem.incerto.registro.${aparelho}`)}</Text>
          </VStack>
        </HStack>
        <HStack>
          <Button onClick={() => void cloud.reconcileRegistration()} disabled={busy}>
            {cloud.busy === 'reconcileRegistration' ? t('nuvem.verificando') : t('nuvem.verificarRegistro')}
          </Button>
        </HStack>
      </Card>
    )
  }
  return (
    <Card busy={busy}>
      <HStack gap={10} align="start">
        <Icon name="exclamationmark.triangle" size={14} color="var(--text-attention, var(--attention))" />
        <VStack grow className="settings-stack" gap={2}>
          <Text size="callout" weight="medium">{t('nuvem.incerto.resultadoTitulo')}</Text>
          <Text size="caption" tone="secondary" role="status">
            {name ? t(`nuvem.incerto.resultadoCom.${aparelho}`, { nome: isolado(name) }) : t(`nuvem.incerto.resultado.${aparelho}`)}
          </Text>
        </VStack>
      </HStack>
      {cloud.recoveryPhase === 'claim' ? <HStack>
        <Button onClick={() => void cloud.reconcileClaim()} disabled={busy}>{cloud.busy === 'reconcileClaim' ? t('nuvem.verificando') : t('nuvem.incerto.verificarPosse')}</Button>
      </HStack> : null}
      {cloud.recoveryPhase !== 'claim' && !publication ? <Text as="div" size="caption" tone="secondary" role="status">{t(`nuvem.incerto.pendencia.${aparelho}`)}</Text> : null}
      {publication && s.computerId ? (
        <HStack gap={8} wrap>
          {attempt ? (
            <Button kind="secondary" onClick={() => void cloud.check()} disabled={busy}>{cloud.busy === 'check' ? t('nuvem.verificando') : t('nuvem.incerto.verificarPedido')}</Button>
          ) : null}
          <Button onClick={() => void cloud.reconcilePin()} disabled={busy}>{cloud.busy === 'reconcilePin' ? t('nuvem.verificando') : t('nuvem.incerto.verificarGravacao')}</Button>
        </HStack>
      ) : null}
    </Card>
  )
}
