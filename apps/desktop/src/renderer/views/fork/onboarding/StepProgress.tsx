import { useT } from '../../../lib/fork-i18n/estado'
import { TOTAL_ONBOARDING_STEPS } from '../../../../shared/fork-onboarding.ts'

export function StepProgress({ step, total = TOTAL_ONBOARDING_STEPS }: { step: 1 | 2 | 3 | 4; total?: number }) {
  const t = useT()
  return (
    <div className="passo-etapas" role="img" aria-label={t('onboarding.etapa', { atual: step, total })}>
      {Array.from({ length: total }, (_, idx) => (
        <span key={idx} className={idx + 1 === step ? 'agora' : undefined} />
      ))}
    </div>
  )
}
