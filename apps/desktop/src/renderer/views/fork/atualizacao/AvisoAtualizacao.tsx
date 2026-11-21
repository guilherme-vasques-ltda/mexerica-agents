import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { ltr } from '@shared/bidi'
import { shouldShowReleaseBanner, type ReleaseNoticeState } from '@shared/fork-release-notice'
import { Button } from '../../../components/Controls'
import { useReleaseNotice } from './use-release-notice'
import './aviso-atualizacao.css'

/**
 * Nível 1 de atualizações: a pílula discreta do canto da janela, só quando há de fato uma versão
 * nova ainda não dispensada (`shouldShowReleaseBanner`). A view pura recebe o estado e as ações —
 * os testes de renderização não precisam da ponte.
 */
export function AvisoAtualizacaoView({ state, onOpenDownload, onDismiss }: {
  state: ReleaseNoticeState | null
  onOpenDownload: () => void
  onDismiss: () => void
}) {
  const t = useT()
  const idioma = useIdioma()
  if (state === null || !shouldShowReleaseBanner(state)) return null
  return (
    <div className="aviso-atualizacao idioma-superficie no-drag" role="status" {...superficieIdioma(idioma)}>
      <span className="aviso-atualizacao-texto">{t('ajustes.atualizacoes.novaVersao', { versao: ltr(state.availableTag ?? '') })}</span>
      <Button kind="secondary" onClick={onOpenDownload}>{t('ajustes.atualizacoes.verBaixar')}</Button>
      <button className="aviso-atualizacao-dispensar" onClick={onDismiss}>{t('ajustes.atualizacoes.dispensar')}</button>
    </div>
  )
}

export function AvisoAtualizacao() {
  const state = useReleaseNotice()
  return (
    <AvisoAtualizacaoView
      state={state}
      onOpenDownload={() => { void window.codync?.releaseNotice?.openDownload() }}
      onDismiss={() => { void window.codync?.releaseNotice?.dismiss() }}
    />
  )
}
