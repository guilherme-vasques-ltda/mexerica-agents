import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { formatarData } from '../../../lib/fork-i18n/index.ts'
import { ltr } from '@shared/bidi'
import type { ReleaseNoticeState } from '@shared/fork-release-notice'
import { Button, CardSection, Switch, ValueRow } from '../../../components/Controls'
import { HStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { Superficie } from '../../settings/rotulo'
import { useReleaseNotice } from './use-release-notice'
import './aviso-atualizacao.css'

/**
 * Ajustes → Geral, seção de atualizações do Nível 1: a versão atual, o botão de procurar e o que a
 * última verificação achou. A view pura recebe o estado e as ações; a montada lê da ponte e some
 * quando o app não tem o serviço (web, demo, builds sem o handler).
 */
export function AtualizacaoAjusteView({ state, onCheck, onOpenDownload, onCheckEnabled }: {
  state: ReleaseNoticeState
  onCheck: () => void
  onOpenDownload: () => void
  onCheckEnabled: (on: boolean) => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const lastChecked = state.lastCheckAt !== null
    ? t('ajustes.atualizacoes.ultima', { quando: formatarData(idioma, state.lastCheckAt, { dateStyle: 'medium', timeStyle: 'short' }) })
    : null
  const versaoAtual = state.currentVersion.startsWith('v') ? state.currentVersion : `v${state.currentVersion}`
  const releaseAvailable = state.availableVersion !== null && state.availableTag !== null && state.releaseUrl !== null
  return (
    <Superficie>
      <CardSection title={t('ajustes.atualizacoes.titulo')}>
        <ValueRow label={t('ajustes.atualizacoes.buscar')} detail={t('ajustes.atualizacoes.buscarDetalhe')}>
          <Switch on={state.checkEnabled} onChange={onCheckEnabled} label={t('ajustes.atualizacoes.buscar')} />
        </ValueRow>
        <ValueRow label={t('ajustes.atualizacoes.versaoAtual')} detail={lastChecked}>
          <HStack as="span" gap={10}>
            <span className="bidi-isolado" dir="ltr">{versaoAtual}</span>
            <Button kind="secondary" onClick={onCheck} disabled={!state.checkEnabled || state.status === 'checking'}>
              {state.status === 'checking' ? t('ajustes.atualizacoes.procurando') : t('ajustes.atualizacoes.procurar')}
            </Button>
          </HStack>
        </ValueRow>
        {releaseAvailable ? (
          <ValueRow label={t('ajustes.atualizacoes.novaVersao', { versao: ltr(state.availableTag ?? '') })}>
            <Button kind="primary" onClick={onOpenDownload}>{t('ajustes.atualizacoes.verBaixar')}</Button>
          </ValueRow>
        ) : null}
      </CardSection>
      {/* Região viva persistente: o leitor de tela anuncia o resultado da checagem sem linha fantasma dentro do cartão. */}
      <div className="aviso-atualizacao-retorno" role="status" aria-live="polite">
        {!releaseAvailable && state.status === 'up-to-date' ? (
          <Text size="footnote" tone="secondary">{t('ajustes.atualizacoes.atualizado')}</Text>
        ) : null}
        {state.status === 'error' ? (
          <Text size="footnote" tone="attention" dir="auto">{t('ajustes.atualizacoes.erro')}</Text>
        ) : null}
      </div>
    </Superficie>
  )
}

export function AtualizacaoAjuste() {
  const state = useReleaseNotice()
  if (!state?.supported) return null
  return (
    <AtualizacaoAjusteView
      state={state}
      onCheck={() => { void window.codync?.releaseNotice?.check() }}
      onOpenDownload={() => { void window.codync?.releaseNotice?.openDownload() }}
      onCheckEnabled={(on) => { void window.codync?.releaseNotice?.setCheckEnabled(on) }}
    />
  )
}
