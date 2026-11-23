import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { formatarNumero } from '../../../lib/fork-i18n'
import type { ReactNode } from 'react'
import type { AttentionQueue, AttentionKind } from '@shared/fork-torre-esperando'
import './esperando-voce.css'

export type AttentionQueueItem = {
  id: string
  title: string | null
  kind: AttentionKind
  onOpen?: () => void
  project?: boolean
  action?: ReactNode
}

export function EsperandoVoceList({ queue, items }: { queue: AttentionQueue | null; items: AttentionQueueItem[] }) {
  const t = useT()
  const idioma = useIdioma()
  const title = t('torre.esperandoVoce.titulo')
  return (
    <section className="torre-group esperando-voce" aria-label={title}>
      <h2 className="torre-group-title">{title} {queue ? <span>{formatarNumero(idioma, queue.items.length)}</span> : null}</h2>
      {!queue ? <p className="esperando-voce-empty" role="status">{t('torre.esperandoVoce.indisponivel')}</p> : null}
      {queue?.coverageGaps.map((gap) => <p className="torre-notice" role="status" key={`${gap.source}:${gap.code}`}>{t('torre.esperandoVoce.lacuna', { fonte: sourceLabel(gap.source, t) })}</p>)}
      {queue && items.length === 0 && !queue.coverageGaps.length ? <p className="esperando-voce-empty">{t('torre.esperandoVoce.vazio')}</p> : null}
      {items.length ? (
        <ul className="esperando-voce-list">
          {items.map((item) => <li key={item.id} className="esperando-voce-row">
            <span className="esperando-voce-content">
              <span className="esperando-voce-kind">{t(kindLabel(item.kind))}</span>
              {item.title ? <bdi className="esperando-voce-title">{item.title}</bdi> : null}
            </span>
            {item.action}
            {item.onOpen ? <button type="button" className="torre-button" onClick={item.onOpen}>{t(item.project ? 'torre.esperandoVoce.abrirProjetos' : 'torre.esperandoVoce.abrir')}</button> : null}
          </li>)}
        </ul>
      ) : null}
      {queue?.resolvedToday.length ? <details className="esperando-voce-resolvidos">
        <summary>{t('torre.esperandoVoce.resolvidos', { n: formatarNumero(idioma, queue.resolvedToday.length) })}</summary>
        <ul className="esperando-voce-list">
          {queue.resolvedToday.map((item) => <li className="esperando-voce-row" key={item.id}>
            <span className="esperando-voce-content">
              <span className="esperando-voce-kind">{t(kindLabel(item.kind))}</span>
              {item.summary ? <bdi className="esperando-voce-title">{item.summary}</bdi> : null}
            </span>
          </li>)}
        </ul>
      </details> : null}
    </section>
  )
}

function sourceLabel(source: string, t: ReturnType<typeof useT>) {
  switch (source) {
    case 'permissions': return t('torre.esperandoVoce.fonte.permissions')
    case 'delegated_permissions': return t('torre.esperandoVoce.fonte.delegated_permissions')
    case 'owner_questions': return t('torre.esperandoVoce.fonte.owner_questions')
    case 'proposals': return t('torre.esperandoVoce.fonte.proposals')
    case 'task_reviews': return t('torre.esperandoVoce.fonte.task_reviews')
    case 'routine_reviews': return t('torre.esperandoVoce.fonte.routine_reviews')
    default: return t('torre.esperandoVoce.fonte.outros')
  }
}

function kindLabel(kind: AttentionQueueItem['kind']) {
  switch (kind) {
    case 'owner_question': return 'torre.esperandoVoce.tipo.owner_question'
    case 'permission': return 'torre.esperandoVoce.tipo.permission'
    case 'delegated_permission': return 'torre.esperandoVoce.tipo.delegated_permission'
    case 'task_proposal': return 'torre.esperandoVoce.tipo.task_proposal'
    case 'task_review': return 'torre.esperandoVoce.tipo.task_review'
    case 'routine_review': return 'torre.esperandoVoce.tipo.routine_review'
    case 'doctor_alert': return 'torre.esperandoVoce.tipo.doctor_alert'
  }
}
