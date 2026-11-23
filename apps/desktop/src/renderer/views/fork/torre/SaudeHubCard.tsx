// Cartão Saúde do Hub: multiplataforma (Desktop e Web), 100% livre de APIs exclusivas do Electron.
// Consulta `donnaHubDoctor` via ponte RPC da plataforma e reconsulta em `donnaDoctorChanged` / `donnaControlTowerChanged`.
import { useEffect, useMemo } from 'react'
import {
  classifyDoctorError,
  parseHubDoctorReport,
  type DoctorActionKind,
  type DoctorCheck,
  type HubDoctorReport,
} from '../../../../shared/fork-doctor.ts'
import { Icon } from '../../../components/Icon'
import { localeDe } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { Observable, useModel } from '../../../lib/observable'
import { hora } from './formato'
import type { TowerLink, TowerSource } from './torre-session'
import './saude-hub-card.css'

export const DOCTOR_REREAD_EVENTS = new Set([
  'donnaDoctorChanged',
  'donnaControlTowerChanged',
  'bot',
  'botDeleted',
  'conversation',
  'entry',
  'session',
  'spaces',
  'whatsappChanged',
  'hello',
  'resync',
])

const REREAD_DELAY = 250

export type SaudeHubState =
  | { kind: 'loading' }
  | { kind: 'ready'; report: HubDoctorReport }
  | { kind: 'hub_offline'; spaceId: string; offlineSince: number | null }
  | { kind: 'unavailable'; reason: 'unsupported' | 'denied' | 'invalid' | 'error' }

interface DoctorOrigin {
  source: TowerSource
  hostId: string
  client: TowerLink
  epoch: number
  gen: number
}

/** Sessão do diagnóstico do Hub, reutilizável no Desktop e na Web sem dependência de Electron. */
export class DoctorSession extends Observable {
  state: SaudeHubState = { kind: 'loading' }

  private origin: DoctorOrigin | null = null
  private hostId: string | null = null
  private spaceId: string | null = null
  private offlineSince: number | null = null
  private gen = 0
  private reads = 0
  private reading = false
  private dirty = false
  private unwatch: (() => void) | null = null
  private stopEvents: (() => void) | null = null
  private rereadTimer: ReturnType<typeof setTimeout> | null = null
  private now: () => number

  constructor(now: () => number = Date.now) {
    super()
    this.now = now
  }

  open(source: TowerSource, spaceId: string) {
    this.release()
    if (this.spaceId !== spaceId) {
      this.state = { kind: 'loading' }
      this.offlineSince = null
    }
    this.spaceId = spaceId
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  close() {
    this.release()
    this.gen++
    this.origin = null
    this.changed()
  }

  reread() {
    const origin = this.origin
    if (origin && this.current(origin)) void this.read(origin)
  }

  private connect(source: TowerSource) {
    this.stopEvents?.()
    this.stopEvents = null
    const gen = ++this.gen
    const hostId = source.hello?.hostId ?? null
    const client = source.connectedClient
    if (hostId && this.hostId && this.hostId !== hostId) {
      this.state = { kind: 'loading' }
    }
    if (hostId) this.hostId = hostId
    if (!hostId || !client || !this.spaceId) {
      this.origin = null
      if (this.offlineSince === null) this.offlineSince = this.now()
      this.state = { kind: 'hub_offline', spaceId: this.spaceId ?? 'personal', offlineSince: this.offlineSince }
      this.changed()
      return
    }
    const origin: DoctorOrigin = { source, hostId, client, epoch: source.principalEpoch, gen }
    this.origin = origin
    this.offlineSince = null
    if (this.state.kind === 'hub_offline') this.state = { kind: 'loading' }
    this.changed()
    this.listen(origin, this.spaceId)
    void this.read(origin)
  }

  private listen(origin: DoctorOrigin, spaceId: string) {
    this.stopEvents = origin.client.inSpace(spaceId).events(
      origin.source.hello?.rev ?? 0,
      'desktop',
      (e) => {
        if (!this.current(origin)) return
        if (!DOCTOR_REREAD_EVENTS.has(e.type)) return
        if ((e.type === 'donnaDoctorChanged' || e.type === 'donnaControlTowerChanged') && e.spaceId !== spaceId) return
        this.scheduleRead(origin)
      },
      () => {
        if (!this.current(origin)) return
        if (this.offlineSince === null) this.offlineSince = this.now()
        this.state = { kind: 'hub_offline', spaceId, offlineSince: this.offlineSince }
        this.changed()
      },
    )
  }

  private watch(source: TowerSource) {
    const origin = this.origin
    const hostId = source.hello?.hostId ?? null
    if (this.hostId && hostId && hostId !== this.hostId) {
      this.hostId = hostId
      this.connect(source)
      return
    }
    if (!origin) {
      if (source.connectedClient && hostId) this.connect(source)
      return
    }
    if (source.connectedClient !== origin.client || source.principalEpoch !== origin.epoch || !hostId) {
      this.connect(source)
    }
  }

  private scheduleRead(origin: DoctorOrigin) {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = setTimeout(() => {
      this.rereadTimer = null
      if (this.current(origin)) void this.read(origin)
    }, REREAD_DELAY)
  }

  private current(origin: DoctorOrigin) {
    return (
      this.origin === origin &&
      this.gen === origin.gen &&
      origin.source.hello?.hostId === origin.hostId &&
      origin.source.connectedClient === origin.client &&
      origin.source.principalEpoch === origin.epoch
    )
  }

  private release() {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = null
    this.stopEvents?.()
    this.stopEvents = null
    this.unwatch?.()
    this.unwatch = null
  }

  private async read(origin: DoctorOrigin) {
    if (this.reading) {
      this.dirty = true
      return
    }
    this.reading = true
    this.dirty = false
    const read = ++this.reads
    const spaceId = this.spaceId!
    try {
      const answer = await origin.client.call('donnaHubDoctor', { spaceId })
      if (!this.current(origin) || read !== this.reads) return
      const report = parseHubDoctorReport(answer, { hubId: origin.hostId, spaceId })
      if (!report) {
        this.state = { kind: 'unavailable', reason: 'invalid' }
      } else {
        this.offlineSince = null
        this.state = { kind: 'ready', report }
      }
    } catch (error) {
      if (!this.current(origin) || read !== this.reads) return
      const kind = classifyDoctorError(error)
      if (kind === 'hub_offline') {
        if (this.offlineSince === null) this.offlineSince = this.now()
        this.state = { kind: 'hub_offline', spaceId, offlineSince: this.offlineSince }
      } else if (kind === 'unsupported') {
        this.state = { kind: 'unavailable', reason: 'unsupported' }
      } else if (kind === 'denied') {
        this.state = { kind: 'unavailable', reason: 'denied' }
      } else {
        this.state = { kind: 'unavailable', reason: 'error' }
      }
    } finally {
      this.reading = false
      if (this.current(origin)) this.changed()
      const latest = this.origin
      if (this.dirty && latest && this.current(latest)) this.scheduleRead(latest)
    }
  }
}

function checkDescription(check: DoctorCheck, t: Tradutor, locale: string): string {
  const n = (check.count ?? 0).toLocaleString(locale)
  switch (check.code) {
    case 'hub_ok':
      return t('torre.saude.codigo.hub_ok')
    case 'hub_offline':
      return t('torre.saude.codigo.hub_offline')
    case 'ai_ok':
      return t('torre.saude.codigo.ai_ok')
    case 'ai_no_active_account':
      return t('torre.saude.codigo.ai_no_active_account')
    case 'ai_all_paused':
      return t('torre.saude.codigo.ai_all_paused')
    case 'ai_rate_limited':
      return t('torre.saude.codigo.ai_rate_limited')
    case 'ai_unavailable':
      return t('torre.saude.codigo.ai_unavailable')
    case 'approvals_ok':
      return t('torre.saude.codigo.approvals_ok')
    case 'approvals_waiting':
      return t('torre.saude.codigo.approvals_waiting', { n })
    case 'approvals_unavailable':
      return t('torre.saude.codigo.approvals_unavailable')
    case 'channels_ok':
      return t('torre.saude.codigo.channels_ok')
    case 'channels_whatsapp_logged_out':
      return t('torre.saude.codigo.channels_whatsapp_logged_out', { n })
    case 'channels_whatsapp_unlinked':
      return t('torre.saude.codigo.channels_whatsapp_unlinked')
    case 'channels_whatsapp_outbox_uncertain':
      return t('torre.saude.codigo.channels_whatsapp_outbox_uncertain', { n })
    case 'channels_unavailable':
      return t('torre.saude.codigo.channels_unavailable')
    case 'mcps_ok':
      return t('torre.saude.codigo.mcps_ok', { n })
    case 'mcps_attention_needed':
      return t('torre.saude.codigo.mcps_attention_needed', { n })
    case 'mcps_unavailable':
      return t('torre.saude.codigo.mcps_unavailable')
    case 'routines_ok':
      return t('torre.saude.codigo.routines_ok', { n })
    case 'routines_paused':
      return t('torre.saude.codigo.routines_paused', { n })
    case 'routines_failed':
      return t('torre.saude.codigo.routines_failed', { n })
    case 'routines_unavailable':
      return t('torre.saude.codigo.routines_unavailable')
    case 'delegation_ok':
      return t('torre.saude.codigo.delegation_ok')
    case 'inter_bot_stalled':
      return t('torre.saude.codigo.inter_bot_stalled', { n })
    case 'delegation_unavailable':
      return t('torre.saude.codigo.delegation_unavailable')
    default:
      return t('torre.saude.codigo.desconhecido', { codigo: check.code })
  }
}

function actionLabel(action: DoctorActionKind, t: Tradutor): string | null {
  switch (action) {
    case 'none':
      return null
    case 'start_hub':
      return t('torre.saude.acao.start_hub')
    case 'connect_ai':
      return t('torre.saude.acao.connect_ai')
    case 'review_waiting':
      return t('torre.saude.acao.review_waiting')
    case 'check_channels':
      return t('torre.saude.acao.check_channels')
    case 'check_mcps':
      return t('torre.saude.acao.check_mcps')
    case 'check_routines':
      return t('torre.saude.acao.check_routines')
    case 'open_torre':
      return t('torre.saude.acao.open_torre')
  }
}

function overallLabel(overall: HubDoctorReport['overall'], t: Tradutor): string {
  switch (overall) {
    case 'healthy':
      return t('torre.saude.overall.healthy')
    case 'waiting_user':
      return t('torre.saude.overall.waiting_user')
    case 'degraded':
      return t('torre.saude.overall.degraded')
    case 'error':
      return t('torre.saude.overall.error')
  }
}

function overallSummary(overall: HubDoctorReport['overall'], t: Tradutor): string {
  switch (overall) {
    case 'healthy':
      return t('torre.saude.overall.healthyTexto')
    case 'waiting_user':
      return t('torre.saude.overall.waiting_userTexto')
    case 'degraded':
      return t('torre.saude.overall.degradedTexto')
    case 'error':
      return t('torre.saude.overall.errorTexto')
  }
}

function areaLabel(area: DoctorCheck['area'], t: Tradutor): string {
  switch (area) {
    case 'hub':
      return t('torre.saude.area.hub')
    case 'ai_engine':
      return t('torre.saude.area.ai_engine')
    case 'approvals':
      return t('torre.saude.area.approvals')
    case 'channels':
      return t('torre.saude.area.channels')
    case 'mcps':
      return t('torre.saude.area.mcps')
    case 'routines':
      return t('torre.saude.area.routines')
    case 'delegation':
      return t('torre.saude.area.delegation')
  }
}

function statusLabel(status: DoctorCheck['status'], t: Tradutor): string {
  switch (status) {
    case 'ok':
      return t('torre.saude.status.ok')
    case 'warning':
      return t('torre.saude.status.warning')
    case 'error':
      return t('torre.saude.status.error')
    case 'unavailable':
      return t('torre.saude.status.unavailable')
  }
}

export function SaudeHubLive({
  source,
  spaceId,
  onRetry,
}: {
  source: TowerSource
  spaceId: string
  onRetry?: () => void
}) {
  const session = useMemo(() => new DoctorSession(), [])
  useModel(session)
  useEffect(() => {
    session.open(source, spaceId)
    return () => session.close()
  }, [session, source, spaceId])
  return <SaudeHubCard state={session.state} onRetry={onRetry ?? (() => session.reread())} />
}

export function SaudeHubCard({
  state,
  onRetry,
}: {
  state: SaudeHubState
  onRetry?: () => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const locale = localeDe(idioma)

  if (state.kind === 'loading') {
    return (
      <section className="saude-card is-loading" aria-label={t('torre.saude.titulo')}>
        <div className="saude-head">
          <Icon name="stethoscope" size={14} />
          <h2 className="saude-title">{t('torre.saude.titulo')}</h2>
          <span className="saude-badge is-loading"><i aria-hidden />{t('torre.saude.carregando')}</span>
        </div>
      </section>
    )
  }

  if (state.kind === 'hub_offline') {
    return (
      <section className="saude-card is-hub-offline" role="status" aria-label={t('torre.saude.titulo')}>
        <div className="saude-head">
          <Icon name="exclamationmark.triangle" size={14} />
          <h2 className="saude-title">{t('torre.saude.titulo')}</h2>
          <span className="saude-badge is-hub_offline" data-saude-overall="hub_offline">
            <i aria-hidden />{t('torre.saude.offline.titulo')}
          </span>
          <div className="saude-meta">
            <span>{t('torre.saude.acao.start_hub')}</span>
            {onRetry ? (
              <button className="torre-button" onClick={onRetry}>{t('torre.saude.tentarDeNovo')}</button>
            ) : null}
          </div>
        </div>
        <p className="saude-summary">{t('torre.saude.offline.texto')}</p>
      </section>
    )
  }

  if (state.kind === 'unavailable') {
    return (
      <section className="saude-card is-unavailable" role="status" aria-label={t('torre.saude.titulo')}>
        <div className="saude-head">
          <Icon name="questionmark.circle" size={14} />
          <h2 className="saude-title">{t('torre.saude.titulo')}</h2>
          <span className="saude-badge is-unavailable" data-saude-overall="unavailable">
            <i aria-hidden />{t('torre.saude.naoInformado')}
          </span>
        </div>
        <p className="saude-summary">{t('torre.saude.naoInformadoTexto')}</p>
      </section>
    )
  }

  const { report } = state
  const badgeClass = `saude-badge is-${report.overall}`
  const cardClass = `saude-card is-${report.overall.replace('_', '-')}`

  return (
    <section className={cardClass} aria-label={t('torre.saude.titulo')}>
      <div className="saude-head">
        <Icon name="stethoscope" size={14} />
        <h2 className="saude-title">{t('torre.saude.titulo')}</h2>
        <span className={badgeClass} data-saude-overall={report.overall}>
          <i aria-hidden />{overallLabel(report.overall, t)}
        </span>
        <div className="saude-meta">
          <span>{t('torre.saude.atualizado', { hora: hora(report.collectedAt, idioma) })}</span>
        </div>
      </div>
      <p className="saude-summary">{overallSummary(report.overall, t)}</p>
      <div className="saude-checks">
        {report.checks.map((check) => {
          const action = actionLabel(check.actionKind, t)
          return (
            <div key={check.id} className="saude-check" data-saude-area={check.area} data-saude-status={check.status}>
              <div className="saude-check-top">
                <span className="saude-check-area">{areaLabel(check.area, t)}</span>
                <span className={`saude-check-status is-${check.status}`}>
                  {statusLabel(check.status, t)}
                </span>
              </div>
              <p className="saude-check-desc">{checkDescription(check, t, locale)}</p>
              {action ? <p className="saude-check-action">{action}</p> : null}
            </div>
          )
        })}
      </div>
    </section>
  )
}
