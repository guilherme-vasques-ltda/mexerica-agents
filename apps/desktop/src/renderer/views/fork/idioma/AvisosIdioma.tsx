import type { AppLanguage } from '@shared/fork-idioma'
import { tradutor } from '../../../lib/fork-i18n/estado'

/**
 * DON-903: os dois avisos honestos fora do pt-BR. O da migração é transitório e sai quando o app inteiro
 * estiver traduzido (docs/guides/idiomas.md); o dos bots fica enquanto eles responderem em português.
 */
export function AvisosIdioma({ idioma, className }: { idioma: AppLanguage; className?: string }) {
  if (idioma === 'pt-BR') return null
  const t = tradutor(idioma)
  return (
    <div className={className} role="note">
      <p data-aviso="migracao">{t('idioma.aviso.migracao')}</p>
      <p data-aviso="bots">{t('idioma.aviso.bots')}</p>
    </div>
  )
}
