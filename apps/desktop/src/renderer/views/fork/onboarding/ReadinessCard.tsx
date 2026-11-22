import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import type { OnboardingStatusPayload } from '../../../../shared/fork-onboarding.ts'
import { summarizeReadiness } from '../../../../shared/fork-onboarding.ts'

export function ReadinessCard({
  status,
  onConnectAi,
  onConfigurePermissions,
}: {
  status: OnboardingStatusPayload | null
  onConnectAi?: () => void
  onConfigurePermissions?: () => void
}) {
  const t = useT()
  if (!status) return null
  const { readyCount, totalCount, items } = summarizeReadiness(status)
  if (totalCount === 0) return null
  const pct = Math.round((readyCount / totalCount) * 100)
  const aiItem = items.find((item) => item.id === 'ai')
  const computerItem = items.find((item) => item.id === 'computer')

  return (
    <section className="onboarding-readiness-card" aria-label={t('onboarding.passo4.prontidaoSubtitulo')}>
      <div className="onboarding-readiness-head">
        <span className="onboarding-readiness-title">{t('onboarding.passo4.prontidaoTitulo', { prontos: readyCount, total: totalCount })}</span>
        <span className="onboarding-readiness-sub">{t('onboarding.passo4.prontidaoSubtitulo')}</span>
      </div>
      <div className="onboarding-readiness-bar" role="progressbar" aria-label={t('onboarding.passo4.prontidaoSubtitulo')} aria-valuenow={readyCount} aria-valuemin={0} aria-valuemax={totalCount}>
        <span style={{ width: `${pct}%` }} />
      </div>
      <ul className="onboarding-readiness-list">
        {items.filter((item) => item.available).map((item) => (
          <li key={item.id} className={item.ready ? 'is-ready' : undefined}>
            <Icon name={item.ready ? 'checkmark.circle.fill' : 'circle.dotted'} size={13} />
            <span>{t(`onboarding.passo4.item.${item.id}`)}</span>
          </li>
        ))}
      </ul>
      <div className="onboarding-readiness-actions">
        {aiItem && !aiItem.ready && onConnectAi ? (
          <Button kind="primary" onClick={onConnectAi}>{t('onboarding.passo4.conectarAgora')}</Button>
        ) : null}
        {computerItem && computerItem.available && !computerItem.ready && onConfigurePermissions ? (
          <Button kind="secondary" onClick={onConfigurePermissions}>{t('onboarding.passo4.configurarPermissoes')}</Button>
        ) : null}
      </div>
    </section>
  )
}
