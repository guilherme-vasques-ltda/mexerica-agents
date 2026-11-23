// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useRef } from 'react'
import { Spinner } from '../components/Controls'
import type { AccountEntry } from './AccountEntryGate'
import { WelcomeActions } from './welcome/WelcomeActions'
import { PassoIdioma, usePassoIdioma } from './fork/idioma/PassoIdioma'
import { OnboardingBrandPanel } from './fork/onboarding/OnboardingBrandPanel'
import { StepProgress } from './fork/onboarding/StepProgress'
import { superficieIdioma, tradutor, useIdioma, useT } from '../lib/fork-i18n/estado'
// D471: a logo com o mascote do pacote oficial (brand/assets/logo/01-logo-mascote, cópias byte a byte).
import logoClaro from '../../../resources/brand/mexerica/mexerica-logo-mascote-claro-pequeno.png'
import logoEscuro from '../../../resources/brand/mexerica/mexerica-logo-mascote-escuro-pequeno.png'
import './fork/idioma/escritas.css'
import './fork/idioma/entrada-marca.css'
import './fork/onboarding/onboarding.css'
import './account-welcome.css'
import './welcome/welcome-calm.css'

/** Entrada calma e ações reais de conta.
 * O conteúdo do app só abre com a conta confirmada pelo processo principal.
 */
export function AccountWelcomeView({ entry }: { entry: Exclude<AccountEntry, 'confirmed'> }) {
  if (entry === 'restoring') return <Restaurando />
  return <WelcomeScreen entry={entry} />
}

function Restaurando() {
  const idioma = useIdioma()
  const t = useT()
  // A sessão salva volta primeiro. Até o processo principal responder, nada do app aparece.
  return (
    <div className="welcome" {...superficieIdioma(idioma)}>
      <div className="welcome-stage" aria-hidden>
        <div className="welcome-backdrop" />
      </div>
      <div className="welcome-restoring idioma-superficie" role="status">
        {/* O texto ao lado já diz o estado; o rótulo do Spinner compartilhado é em inglês. */}
        <span className="welcome-spin" aria-hidden>
          <Spinner size={24} />
        </span>
        <p>{t('entrada.restaurando')}</p>
      </div>
    </div>
  )
}

function WelcomeScreen({ entry }: { entry: Exclude<AccountEntry, 'confirmed' | 'restoring'> }) {
  // DON-903 (D362/24A): o idioma é o primeiro passo. Não decide o portão.
  const idioma = usePassoIdioma()
  const lingua = useIdioma()
  const column = useRef<HTMLElement>(null)
  useEffect(() => {
    if (idioma.concluido) column.current?.focus({ preventScroll: true })
  }, [idioma.concluido])
  if (idioma.pendente && entry === 'signed_out') {
    return <PassoIdioma sugerido={window.codync.suggestedLanguage} onContinuar={idioma.concluir} />
  }

  // Fatia 1 (D454): a conta segue o idioma escolhido no passo 1.
  const pt = tradutor(lingua)
  return (
    <div className="welcome welcome-calm idioma-superficie" {...superficieIdioma(lingua)}>
      <div className="welcome-stage" aria-hidden><div className="welcome-backdrop" /></div>
      <div className="onboarding-welcome-grid">
        <OnboardingBrandPanel />
        <div className="onboarding-welcome-right">
          <div className="onboarding-welcome-progress">
            <StepProgress step={2} />
          </div>
          <main ref={column} className="welcome-column" tabIndex={-1}>
            <h1 className="welcome-sr-only">{pt('entrada.boasVindas')}</h1>
            <header className="welcome-brand">
              {/* O tema escolhe a versão pelo CSS (data-theme), sem ler a aparência aqui. */}
              <img className="welcome-logo is-claro" src={logoClaro} alt={pt('entrada.marca')} />
              <img className="welcome-logo is-escuro" src={logoEscuro} alt={pt('entrada.marca')} />
            </header>
            <WelcomeActions entry={entry} />
          </main>
        </div>
      </div>
    </div>
  )
}
