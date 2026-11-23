import { Button } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'

export function ComposerSpotlight({
  open,
  onDismiss,
  onSkipTour,
}: {
  open: boolean
  onDismiss: () => void
  onSkipTour: () => void
}) {
  const t = useT()
  if (!open) return null
  return (
    <div className="onboarding-spotlight" role="dialog" aria-label={t('onboarding.passo4.spotlightTitulo')}>
      <div className="onboarding-spotlight-head">
        <span className="onboarding-spotlight-badge">{t('onboarding.passo4.spotlightBadge')}</span>
      </div>
      <p className="onboarding-spotlight-title">{t('onboarding.passo4.spotlightTitulo')}</p>
      <p className="onboarding-spotlight-desc">{t('onboarding.passo4.spotlightDesc')}</p>
      <div className="onboarding-spotlight-actions">
        <Button kind="primary" onClick={onDismiss}>{t('onboarding.passo4.spotlightEntendi')}</Button>
        <button type="button" className="onboarding-spotlight-skip" onClick={onSkipTour}>
          {t('onboarding.passo4.spotlightPular')}
        </button>
      </div>
      <div className="onboarding-spotlight-arrow" aria-hidden="true" />
    </div>
  )
}
