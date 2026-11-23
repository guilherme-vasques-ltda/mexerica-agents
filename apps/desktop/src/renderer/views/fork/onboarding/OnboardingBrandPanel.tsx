import { useT } from '../../../lib/fork-i18n/estado'
import mascote from '../../../../../resources/brand/mexerica/mexerica-mascote-256.png'

/**
 * Coluna esquerda calma da etapa 2:
 * exibe o mascote e dois chips ilustrativos sem bypass de conta e oculta automaticamente em janelas estreitas.
 */
export function OnboardingBrandPanel() {
  const t = useT()
  return (
    <aside className="onboarding-brand-panel" aria-hidden="true">
      <div className="onboarding-brand-card">
        <img className="onboarding-brand-mascot" src={mascote} alt="" />
        <div className="onboarding-brand-chips">
          <div className="onboarding-preview-chip">{t('onboarding.boasVindas.chip1')}</div>
          <div className="onboarding-preview-chip is-approval">
            <span>{t('onboarding.boasVindas.chip2')}</span>
            <span className="onboarding-preview-approve">{t('onboarding.boasVindas.aprovar')}</span>
          </div>
        </div>
      </div>
      <p className="onboarding-brand-privacy">{t('onboarding.boasVindas.privacidade')}</p>
    </aside>
  )
}
