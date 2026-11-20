import { type MouseEvent } from 'react'
import type { BrowserLiveBridge } from '@shared/fork-browser-live'
import type { BrowserScope } from '@shared/fork-browser'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { aceitaClique, atrasoDoQuadro, chaveDoMotivo, faixaDoEstado } from './estado'
import { useAoVivo } from './useAoVivo'
import './navegador-ao-vivo.css'

/**
 * Painel do navegador do bot ao vivo (fatia 2 B2, desenho em docs/propostas/navegador-visao-ao-vivo-20261116):
 * faixa que diz quem controla, o quadro que o Hub manda e, com a pessoa no controle, clique no quadro.
 * Na v1 não há digitação, colagem nem teclas pela visão ao vivo (decisão do dono, 17/11/2026).
 */
export function NavegadorAoVivo({ bridge, scope, session, target, conexao, botName, onClose }: {
  bridge: BrowserLiveBridge
  scope: BrowserScope
  session: string
  target: string
  conexao: string
  botName: string
  onClose: () => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const { canvas, state, idadeMs, aviso, temQuadro, assumir, clicar } = useAoVivo({ bridge, scope, session, target, conexao })
  const faixa = faixaDoEstado(state)
  const atraso = atrasoDoQuadro(idadeMs)
  const clicavel = aceitaClique(faixa, atraso, temQuadro)
  const fechada = faixa.kind === 'fechada'

  const aoClicar = (event: MouseEvent<HTMLDivElement>) => {
    if (!clicavel) return
    const caixa = event.currentTarget.getBoundingClientRect()
    void clicar(event.clientX - caixa.left, event.clientY - caixa.top, caixa.width, caixa.height)
  }

  return <section className={`aovivo is-${faixa.kind}`} aria-label={t('ajustes.navegadorAoVivo.titulo', { bot: botName })} {...superficieIdioma(idioma)}>
    <HStack gap={8} className="aovivo-topo">
      <Text size="title3" weight="semibold" truncate grow>{t('ajustes.navegadorAoVivo.titulo', { bot: botName })}</Text>
      {!fechada && faixa.kind !== 'abrindo' && <span className="aovivo-selo"><Icon name="record.circle" size={11} weight="semibold" />{t('ajustes.navegadorAoVivo.aoVivo')}</span>}
      <Button kind="secondary" onClick={onClose}>{t('ajustes.navegadorAoVivo.fechar')}</Button>
    </HStack>

    <div className="aovivo-faixa" role="status">
      <HStack gap={8} wrap>
        <Icon name={faixa.kind === 'pessoa' ? 'hand.point.up.left' : fechada ? 'wifi.slash' : 'cursorarrow.motionlines'} size={13} weight="semibold" />
        <Text size="callout" weight="semibold" grow>
          {faixa.kind === 'abrindo' ? t('ajustes.navegadorAoVivo.abrindo', { bot: botName })
            : faixa.kind === 'pessoa' ? t('ajustes.navegadorAoVivo.pessoaControla', { bot: botName })
            : faixa.kind === 'fechada' ? t(chaveDoMotivo(faixa.reason))
            : t('ajustes.navegadorAoVivo.botControla', { bot: botName })}
        </Text>
        {(faixa.kind === 'bot' || faixa.kind === 'assumindo') && <Button disabled={faixa.kind === 'assumindo'} onClick={() => void assumir('person')}>{faixa.kind === 'assumindo' ? t('ajustes.navegadorAoVivo.assumindo') : t('ajustes.navegadorAoVivo.assumir')}</Button>}
        {faixa.kind === 'pessoa' && <Button onClick={() => void assumir('agent')}>{t('ajustes.navegadorAoVivo.devolver', { bot: botName })}</Button>}
      </HStack>
    </div>

    {atraso !== null && !fechada && <div className="aovivo-lento" role="status"><Icon name="exclamationmark.triangle" size={12} weight="semibold" /><Text size="callout">{t('ajustes.navegadorAoVivo.lento', { segundos: atraso })}</Text></div>}

    <div
      className={`aovivo-quadro${clicavel ? ' is-clicavel' : ''}${fechada ? ' is-fechado' : ''}`}
      role="group"
      aria-label={t('ajustes.navegadorAoVivo.quadro', { bot: botName })}
      onClick={aoClicar}
    >
      <canvas ref={canvas} className="aovivo-canvas" aria-hidden="true" />
      {!temQuadro && !fechada && <VStack gap={6} className="aovivo-espera"><Text size="callout" tone="secondary">{t(faixa.kind === 'abrindo' ? 'ajustes.navegadorAoVivo.abrindo' : 'ajustes.navegadorAoVivo.semQuadro', { bot: botName })}</Text></VStack>}
    </div>

    {!fechada && <Text size="caption" tone="secondary" className="aovivo-rodape">
      {aviso === 'assumir' ? t('ajustes.navegadorAoVivo.avisoAssumir')
        : aviso === 'clique' ? t('ajustes.navegadorAoVivo.avisoClique')
        : faixa.kind === 'pessoa' ? t('ajustes.navegadorAoVivo.dicaClique')
        : t('ajustes.navegadorAoVivo.somenteVer')}
    </Text>}
  </section>
}
