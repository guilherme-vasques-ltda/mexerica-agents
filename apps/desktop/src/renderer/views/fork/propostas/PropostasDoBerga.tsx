import { useEffect, useId, useMemo, useState } from 'react'
import { isGroup } from '@shared/models'
import {
  activeSpaceAndPrincipal,
  countActiveProposalTasks,
  includedPendingItemIds,
  isProposalItemOpen,
  resolveItemStartBadge,
  taskStateBadge,
  visibleProposals,
  type ProposalItem,
  type TaskProposal,
  type TaskProposalsSnapshot,
} from '@shared/fork-propostas'
import { Icon } from '../../../components/Icon'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import type { AppLanguage } from '../../../../shared/fork-idioma'
import { listEqual, useModel, useSelector } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import type { TowerSource } from '../torre/torre-session'
import { PropostasSession, type DecideState, type ProposalsFailure } from './propostas-session'
import './propostas.css'

const sessions = new WeakMap<BotStore, PropostasSession>()

/** Uma sessão de propostas por `BotStore`, apenas em memória. */
export function propostasSessionFor(store: BotStore): PropostasSession {
  let s = sessions.get(store)
  if (!s || s.phase.kind === 'moved') {
    s = new PropostasSession()
    sessions.set(store, s)
  }
  return s
}

export interface PropostasCardProps {
  snapshot: TaskProposalsSnapshot | null
  write: DecideState | null
  stale: boolean
  rereadFailed: boolean
  canDecide: boolean
  readError?: ProposalsFailure | null
  retryingRead?: boolean
  botName: (botId: string | null) => string
  t: Tradutor
  idioma: AppLanguage
  onApprove: (proposal: TaskProposal, omitItemIds: string[]) => void
  onReject: (proposal: TaskProposal) => void
  onRetry: () => void
  onDismissWrite: () => void
  onRetryRead?: () => void
}

/** Cartão recolhível com propostas pendentes e tarefas abertas de propostas aprovadas. */
export function PropostasCard({
  snapshot,
  write,
  stale,
  rereadFailed,
  canDecide,
  readError = null,
  retryingRead = false,
  botName,
  t,
  idioma,
  onApprove,
  onReject,
  onRetry,
  onDismissWrite,
  onRetryRead,
}: PropostasCardProps) {
  const [collapsed, setCollapsed] = useState(false)
  const [omittedById, setOmittedById] = useState<Record<string, ReadonlySet<string>>>({})
  const bodyId = useId()

  const proposals = useMemo(() => visibleProposals(snapshot), [snapshot])
  const pendingCount = useMemo(() => proposals.filter((p) => p.state === 'pending').length, [proposals])
  const openTasksCount = useMemo(
    () => proposals.filter((p) => p.state === 'approved').reduce((n, p) => n + p.items.filter(isProposalItemOpen).length, 0),
    [proposals],
  )
  const activeCounts = useMemo(
    () => countActiveProposalTasks(snapshot?.proposals ?? []),
    [snapshot],
  )

  if (proposals.length === 0 && !write && !readError) return null

  const toggleItem = (proposalId: string, itemId: string, include: boolean) => {
    setOmittedById((prev) => {
      const next = new Set(prev[proposalId] ?? [])
      if (include) next.delete(itemId)
      else next.add(itemId)
      return { ...prev, [proposalId]: next }
    })
  }

  return (
    <div className="prp-wrap">
      <section className="prp-card idioma-superficie" aria-label={t('propostas.regiao')} {...superficieIdioma(idioma)}>
        <div className="prp-head">
          <button
            type="button"
            className="prp-toggle"
            aria-expanded={!collapsed}
            aria-controls={bodyId}
            aria-label={t(collapsed ? 'propostas.expandir' : 'propostas.recolher')}
            onClick={() => setCollapsed((c) => !c)}
          >
            <span className="prp-toggle-icon" aria-hidden>
              <Icon name="checklist" size={14} weight="medium" />
            </span>
            <span className="prp-title">{t('propostas.titulo')}</span>
            <span className="prp-summary">
              {pendingCount > 0 ? (
                <span className="prp-pill is-pending">{t.plural('propostas.resumoPendentes', pendingCount)}</span>
              ) : null}
              {openTasksCount > 0 ? (
                <span className="prp-pill">{t.plural('propostas.resumoAbertas', openTasksCount)}</span>
              ) : null}
            </span>
            <span className="prp-chevron" aria-hidden>
              <Icon name={collapsed ? 'chevron.right' : 'chevron.down'} size={11} weight="semibold" />
            </span>
          </button>
        </div>

        {!collapsed ? (
          <div className="prp-body" id={bodyId}>
            <WriteNotice write={write} stale={stale} t={t} onRetry={onRetry} onDismiss={onDismissWrite} />
            {readError ? (
              <div className="prp-banner is-error" role="alert">
                <Icon name="exclamationmark.triangle" size={13} />
                <p>
                  {t(
                    readError === 'lostAccess'
                      ? 'propostas.erro.semAcesso'
                      : readError === 'offline'
                        ? 'propostas.semConexao'
                        : 'propostas.erro.leitura',
                  )}
                </p>
                {onRetryRead ? (
                  <div className="prp-banner-actions">
                    <button
                      type="button"
                      className="prp-btn is-primary"
                      disabled={retryingRead}
                      aria-busy={retryingRead || undefined}
                      onClick={onRetryRead}
                    >
                      {t('propostas.tentarDeNovo')}
                    </button>
                  </div>
                ) : null}
              </div>
            ) : rereadFailed && onRetryRead ? (
              <div className="prp-banner" role="status">
                <Icon name={stale ? 'exclamationmark.triangle' : 'info.circle'} size={13} />
                <p>{t('propostas.releituraFalhou')}</p>
                <div className="prp-banner-actions">
                  <button
                    type="button"
                    className="prp-btn is-primary"
                    disabled={retryingRead}
                    aria-busy={retryingRead || undefined}
                    onClick={onRetryRead}
                  >
                    {t('propostas.tentarDeNovo')}
                  </button>
                </div>
              </div>
            ) : stale ? (
              <div className="prp-banner" role="status">
                <Icon name="exclamationmark.triangle" size={13} />
                <p>{t('propostas.semConexao')}</p>
              </div>
            ) : rereadFailed ? (
              <div className="prp-banner" role="status">
                <Icon name="info.circle" size={13} />
                <p>{t('propostas.releituraFalhou')}</p>
              </div>
            ) : null}

            {proposals.map((proposal) => {
              if (proposal.state === 'pending') {
                const omitted = omittedById[proposal.id] ?? new Set<string>()
                const includedIds = includedPendingItemIds(proposal, omitted)
                const sendingThis = write?.status === 'sending' && write.proposalId === proposal.id
                const locked = !canDecide || write?.status === 'sending' || write?.status === 'uncertain'
                return (
                  <article key={proposal.id} className="prp-proposal" aria-label={proposal.title}>
                    <div className="prp-proposal-head">
                      <h3 className="prp-proposal-title">{proposal.title}</h3>
                      {proposal.projectName ? <span className="prp-project">{proposal.projectName}</span> : null}
                    </div>
                    <p className="prp-note" role="note">
                      <Icon name="info.circle" size={13} />
                      <span>{t('propostas.avisoAntesOk')}</span>
                    </p>
                    <ul className="prp-items">
                      {proposal.items.map((item) => (
                        <PendingProposalItem
                          key={item.id}
                          item={item}
                          checked={item.included && !omitted.has(item.id)}
                          locked={locked}
                          botLabel={botName(item.assigneeBotId)}
                          t={t}
                          onToggle={(include) => toggleItem(proposal.id, item.id, include)}
                        />
                      ))}
                    </ul>
                    <div className="prp-foot">
                      {snapshot ? (
                        <p className="prp-rule">
                          {t('propostas.regraInicio', { total: snapshot.limits.total, porBot: snapshot.limits.perBot })}
                        </p>
                      ) : null}
                      <div className="prp-actions">
                        <button
                          type="button"
                          className="prp-btn"
                          disabled={locked}
                          onClick={() => onReject(proposal)}
                        >
                          {t('propostas.recusar')}
                        </button>
                        <button
                          type="button"
                          className="prp-btn is-primary"
                          disabled={locked || includedIds.length === 0}
                          aria-busy={sendingThis || undefined}
                          onClick={() => onApprove(proposal, [...omitted])}
                        >
                          {sendingThis ? t('propostas.enviando') : t('propostas.aprovar')}
                        </button>
                      </div>
                    </div>
                  </article>
                )
              }

              const approvedItems = proposal.items.filter((it) => it.included)
              return (
                <article key={proposal.id} className="prp-proposal" aria-label={proposal.title}>
                  <div className="prp-proposal-head">
                    <h3 className="prp-proposal-title">{proposal.title}</h3>
                    {proposal.projectName ? <span className="prp-project">{proposal.projectName}</span> : null}
                    <span className="prp-badge">{t('propostas.aprovada')}</span>
                  </div>
                  <ul className="prp-items">
                    {approvedItems.map((item) => {
                      const taskBadge = item.taskState ? taskStateBadge(item.taskState) : null
                      const startBadge = snapshot ? resolveItemStartBadge(item, snapshot.limits, activeCounts) : null
                      return (
                        <li key={item.id} className="prp-item">
                          <span className="prp-item-main">
                            <span className="prp-assignee">{botName(item.assigneeBotId)}</span>
                            <span className="prp-item-title">{item.title}</span>
                          </span>
                          <span className="prp-badges">
                            {taskBadge ? (
                              <span className={`prp-badge is-${taskBadge.tone}`}>{t(taskBadge.key)}</span>
                            ) : null}
                            {startBadge ? (
                              <span className={`prp-badge is-${startBadge.tone}`}>{t(startBadge.key)}</span>
                            ) : null}
                          </span>
                        </li>
                      )
                    })}
                  </ul>
                </article>
              )
            })}
          </div>
        ) : null}
      </section>
    </div>
  )
}

function PendingProposalItem({
  item,
  checked,
  locked,
  botLabel,
  t,
  onToggle,
}: {
  item: ProposalItem
  checked: boolean
  locked: boolean
  botLabel: string
  t: Tradutor
  onToggle: (include: boolean) => void
}) {
  const detailsId = useId()
  const description = item.description.trim()
  const acceptance = item.acceptance.trim()
  const hasDetails = description.length > 0 || acceptance.length > 0
  return (
    <li className={`prp-item is-pending${checked ? '' : ' is-omitted'}`}>
      <label className="prp-check">
        <input
          type="checkbox"
          checked={checked}
          disabled={!item.included || locked}
          aria-label={t('propostas.incluirLinha', { titulo: item.title })}
          aria-describedby={hasDetails ? detailsId : undefined}
          onChange={(e) => onToggle(e.target.checked)}
        />
        <span className="prp-item-main">
          <span className="prp-assignee">{botLabel}</span>
          <span className="prp-item-title">{item.title}</span>
        </span>
      </label>
      {hasDetails ? (
        <div className="prp-item-details" id={detailsId}>
          {description ? <p className="prp-item-desc">{description}</p> : null}
          {acceptance ? (
            <p className="prp-item-acceptance">
              <span className="prp-item-detail-label">{t('propostas.aceite')}</span>
              <span>{acceptance}</span>
            </p>
          ) : null}
        </div>
      ) : null}
    </li>
  )
}

function WriteNotice({
  write,
  stale,
  t,
  onRetry,
  onDismiss,
}: {
  write: DecideState | null
  stale: boolean
  t: Tradutor
  onRetry: () => void
  onDismiss: () => void
}) {
  if (!write) return null
  if (write.status === 'sending') {
    return (
      <div className="prp-banner" role="status">
        <Icon name="arrow.triangle.2.circlepath" size={13} />
        <p>{t('propostas.enviando')}</p>
      </div>
    )
  }
  if (write.status === 'uncertain') {
    return (
      <div className="prp-banner is-uncertain" role="alert">
        <Icon name="exclamationmark.triangle" size={13} />
        <p>{t('propostas.erro.incerto')}</p>
        <div className="prp-banner-actions">
          <button type="button" className="prp-btn is-primary" disabled={stale} onClick={onRetry}>{t('propostas.tentarDeNovo')}</button>
          <button type="button" className="prp-btn" onClick={onDismiss}>{t('propostas.descartar')}</button>
        </div>
      </div>
    )
  }
  const message = write.status === 'conflict'
    ? (write.detail ? t('propostas.erro.conflitoDetalhe', { motivo: write.detail }) : t('propostas.erro.conflito'))
    : write.status === 'decided'
      ? (write.detail ? t('propostas.erro.decididaDetalhe', { motivo: write.detail }) : t('propostas.erro.decidida'))
      : (write.detail ? t('propostas.erro.recusadaDetalhe', { motivo: write.detail }) : t('propostas.erro.recusada'))

  return (
    <div className="prp-banner is-error" role="alert">
      <Icon name="exclamationmark.triangle" size={13} />
      <p>{message}</p>
      <div className="prp-banner-actions">
        <button type="button" className="prp-btn" onClick={onDismiss}>{t('propostas.fecharAviso')}</button>
      </div>
    </div>
  )
}

/** Cartão conectado à sessão de propostas quando a conversa aberta é a principal do Space ativo. */
export function PropostasDoBerga({
  store,
  botId,
  openSession,
}: {
  store: BotStore
  botId: string
  openSession: string | null
}) {
  const t = useT()
  const idioma = useIdioma()
  useSelector(store, (s) => [s.spaceId, s.spaces, s.hello, s.bots, s.mismatch?.kind ?? null], listEqual)
  useModel(store.principal)

  const bot = store.bots.get(botId) ?? null
  const { spaceId, principalBotId } = activeSpaceAndPrincipal(store)
  const isPrincipalChat = (
    openSession === null
    && !store.mismatch
    && !!spaceId
    && !!principalBotId
    && botId === principalBotId
    && (!bot || !isGroup(bot))
  )

  const holder = useMemo(() => ({}), [])
  const session = useMemo(() => propostasSessionFor(store), [store])
  useModel(session)

  useEffect(() => {
    if (!isPrincipalChat || !spaceId) {
      session.close(holder)
      return
    }
    session.open(store as unknown as TowerSource, spaceId, holder)
    return () => session.close(holder)
  }, [session, store, isPrincipalChat, spaceId, holder])

  if (!isPrincipalChat || session.phase.kind === 'unavailable' || session.phase.kind === 'moved') return null

  const botName = (id: string | null) => {
    if (!id) return t('propostas.semResponsavel')
    return store.bots.get(id)?.name ?? t('propostas.responsavelIndisponivel')
  }

  return (
    <PropostasCard
      snapshot={session.snapshot}
      write={session.write}
      stale={session.stale}
      rereadFailed={session.rereadFailed}
      canDecide={session.canDecide}
      readError={session.phase.kind === 'error' ? session.phase.reason : null}
      retryingRead={session.retryingRead}
      botName={botName}
      t={t}
      idioma={idioma}
      onApprove={(proposal, omitItemIds) => { void session.decide(proposal, 'approve', omitItemIds) }}
      onReject={(proposal) => { void session.decide(proposal, 'reject') }}
      onRetry={() => { void session.retry() }}
      onDismissWrite={() => session.dismissWrite()}
      onRetryRead={session.canRetryRead ? () => session.retryRead() : undefined}
    />
  )
}
