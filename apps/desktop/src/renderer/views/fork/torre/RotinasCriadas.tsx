import { useEffect, useRef, useState } from 'react'
import type { TowerSnapshot } from '@shared/fork-torre'
import { parseRoutineReviewResult, type ReviewRoutine, type RoutineCreationReview, type RoutineReviewChoice } from '@shared/fork-torre-rotinas'
import { Button } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n'
import './rotinas-criadas.css'

function RoutineCard({ item, hubId, spaceId, live, onReview }: {
  item: RoutineCreationReview; hubId: string; spaceId: string; live: boolean; onReview?: ReviewRoutine | null
}) {
  const t = useT()
  const idioma = useIdioma()
  const date = (ms: number) => formatarData(idioma, ms, { dateStyle: 'short', timeStyle: 'short' })
  const [busy, setBusy] = useState(false)
  const [decision, setDecision] = useState<'kept' | 'cancelled' | 'enabled' | null>(null)
  const [error, setError] = useState(false)
  const mounted = useRef(false)
  const sending = useRef(false)
  const current = useRef({ live, onReview })
  current.current = { live, onReview }
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])
  const decide = async (choice: RoutineReviewChoice) => {
    const review = current.current.onReview
    if (!current.current.live || !review || sending.current || decision) return
    sending.current = true
    setBusy(true)
    setError(false)
    try {
      const answer = await review({ hubId, spaceId, entryId: item.entryId, entryRev: item.entryRev, choice,
        ...(choice === 'enable' && item.routineRevision ? { routineRevision: item.routineRevision } : {}) })
      const result = parseRoutineReviewResult(answer, item, choice)
      if (mounted.current && current.current.live) setDecision(result.decision)
    } catch {
      if (mounted.current) setError(true)
    } finally {
      sending.current = false
      if (mounted.current) setBusy(false)
    }
  }
  const cancelled = decision === 'cancelled'
  return (
    <article className="rotinas-card" aria-label={t('rotinas.criadas.rotinaAria', { nome: item.name })} aria-busy={busy}>
      <div className="rotinas-head"><h3>{item.name}</h3><span className={`torre-pill is-${!cancelled && item.status === 'active' ? 'idle' : 'plain'}`}>{cancelled ? t('rotinas.criadas.cancelada') : item.status === 'active' ? t('rotinas.criadas.ativa') : t('rotinas.criadas.pausada')}</span></div>
      <p>{t('rotinas.criadas.criadaPor', { nome: item.botName })} · {date(item.createdAt)}</p>
      <p>{item.when}{!cancelled && item.nextRunAt !== null ? ` · ${t('rotinas.criadas.proxima', { data: date(item.nextRunAt) })}` : ''}</p>
      {!decision ? <>
        <p>{live ? (item.externalReleasePending ? t('rotinas.criadas.externaNota') : item.status === 'active' ? t('rotinas.criadas.ativaNota') : t('rotinas.criadas.pausadaNota')) : t('rotinas.criadas.offlineNota')}</p>
        <p className="rotinas-note">{t(item.externalReleasePending ? 'rotinas.criadas.ligarExplicacao' : 'rotinas.criadas.explicacao')}</p>
        <div className="rotinas-actions">
          <Button kind="secondary" disabled={!live || !onReview || busy} onClick={() => void decide(item.externalReleasePending ? 'enable' : 'keep')}>{t(item.externalReleasePending ? 'rotinas.criadas.ligar' : 'rotinas.criadas.manter')}</Button>
          <Button kind="secondary" disabled={!live || !onReview || busy} onClick={() => void decide('cancel')}>{t('rotinas.criadas.cancelar')}</Button>
          {busy ? <span role="status">{t('rotinas.criadas.enviando')}</span> : null}
        </div>
        {error ? <p className="rotinas-error" role="alert">{t('rotinas.criadas.erro')}</p> : null}
      </> : <p role="status">{decision === 'kept' ? t('rotinas.criadas.mantida') : decision === 'enabled' ? t('rotinas.criadas.ligada') : t('rotinas.criadas.canceladaNota')}</p>}
    </article>
  )
}

/** Sem I/O ou regra de cota: exibe a projeção autorizada pelo Hub, inclusive quando o bot está livre. */
export function RotinasCriadas({ snapshot, live, onReview }: { snapshot: TowerSnapshot; live: boolean; onReview?: ReviewRoutine | null }) {
  const t = useT()
  const idioma = useIdioma()
  const date = (ms: number) => formatarData(idioma, ms, { dateStyle: 'short', timeStyle: 'short' })
  const title = t('rotinas.criadas.titulo')
  const observed = snapshot.routineCreations
  if (observed.availability !== 'available') return (
    <section className="torre-group idioma-superficie" {...superficieIdioma(idioma)} aria-label={title}>
      <h2 className="torre-group-title">{title}</h2>
      <p className="rotinas-unavailable"><span className="torre-pill is-plain">{t('rotinas.criadas.naoInformado')}</span> {t('rotinas.criadas.indisponivel')}</p>
    </section>
  )
  const { items, blocked } = observed.value
  if (!items.length && !blocked.length) return null
  return (
    <section className="torre-group idioma-superficie" {...superficieIdioma(idioma)} aria-label={title}>
      <h2 className="torre-group-title">{title} <span>{live ? t('rotinas.criadas.avisos') : t('rotinas.criadas.ultimaAtualizacao')}</span></h2>
      <div className="rotinas-list">
        {items.map((item) => <RoutineCard key={JSON.stringify([item.entryId, item.entryRev])} item={item} hubId={snapshot.hubId} spaceId={snapshot.spaceId} live={live} onReview={onReview} />)}
        {blocked.map((block) => <article key={block.botId} className="rotinas-card" aria-label={t('rotinas.criadas.limiteAria', { nome: block.botName })}>
          <div className="rotinas-head"><h3>{block.botName}</h3><span className="torre-pill is-plain">{t('rotinas.criadas.limiteAtingido')}</span></div>
          <p>{t('rotinas.criadas.uso', { usadas: formatarNumero(idioma, block.used), limite: formatarNumero(idioma, block.limit) })} · {block.limit > 0 ? t('rotinas.criadas.tentativa', { data: date(block.retryAt) }) : t('rotinas.criadas.bloqueadas')}</p>
          <p>{block.reason}</p>
          <p>{block.howToIncrease}</p>
        </article>)}
      </div>
    </section>
  )
}
