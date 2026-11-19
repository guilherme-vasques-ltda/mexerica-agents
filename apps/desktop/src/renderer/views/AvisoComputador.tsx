import { useEffect, useId, useState, type ReactNode } from 'react'
import { Spinner } from '../components/Controls'
import { Icon } from '../components/Icon'
import { HStack, VStack } from '../components/Stack'
import { Text } from '../components/Text'
import { proximaFase, VOLTOU_MS, type AvisoDesatualizado, type FaseAviso } from '../lib/fork-aviso-desatualizado.ts'
import type { Tradutor } from '../lib/fork-i18n/index.ts'
import { comNos, no } from './settings/rotulo'
import './aviso-computador.css'

/** A fase do aviso ao longo do tempo: a confirmação de que voltou ao normal some sozinha. */
export function useFaseAviso(aviso: AvisoDesatualizado | null, conectado: boolean): FaseAviso {
  const [fase, setFase] = useState<FaseAviso>(aviso ? 'desatualizado' : 'oculto')
  const proxima = proximaFase(fase, aviso, conectado)
  if (proxima !== fase) setFase(proxima)
  useEffect(() => {
    if (proxima !== 'voltou') return
    const timer = setTimeout(() => setFase('oculto'), VOLTOU_MS)
    return () => clearTimeout(timer)
  }, [proxima])
  return proxima
}

export interface AvisoComputadorProps {
  t: Tradutor
  fase: Exclude<FaseAviso, 'oculto'>
  /** O nome do computador, já isolado para a direção do texto (`<bdi dir="auto">`). */
  host: ReactNode
  aviso: AvisoDesatualizado | null
  /** O texto de "o que fazer" deste caso (vem do conector: depende do sistema e de quem pode atualizar). */
  como: string
  /** O botão de atualizar, quando este app consegue começar a atualização. */
  acao?: ReactNode
}

/**
 * O aviso calmo de um computador (ou deste app) que precisa de atualização: o que aconteceu, o que continua, o que
 * fica parado e o que fazer. Informativo, sem vermelho: ocupa o lugar do computador, nunca a tela inteira.
 */
export function AvisoComputador({ t, fase, host, aviso, como, acao }: AvisoComputadorProps) {
  const titulo = useId()
  const par = { host: no('host') }
  if (fase === 'voltou' || !aviso) {
    return (
      <div className="aviso-computador aviso-computador-voltou" role="status">
        <HStack align="flex-start" gap={8}>
          <Icon name="checkmark.circle" size={14} color="var(--success)" />
          <Text as="div" size="footnote" tone="text">{comNos(t('navegacao.janela.desatualizado.voltou', par), { host })}</Text>
        </HStack>
      </div>
    )
  }
  const doApp = aviso.alvo === 'app'
  return (
    <section className="aviso-computador" role="status" aria-labelledby={titulo}>
      <HStack align="flex-start" gap={10}>
        <Icon name="arrow.down.circle" size={15} color="var(--ink-2)" />
        <Text as="div" size="subheadline" weight="semibold" tone="text" id={titulo}>
          {comNos(doApp ? t('navegacao.janela.desatualizado.tituloApp', par) : t('navegacao.janela.desatualizado.tituloComputador', par), { host })}
        </Text>
      </HStack>
      <dl className="aviso-computador-itens">
        <div>
          <dt>{t('navegacao.janela.desatualizado.continuaRotulo')}</dt>
          <dd>{t('navegacao.janela.desatualizado.continuaTexto')}</dd>
        </div>
        <div>
          <dt>{t('navegacao.janela.desatualizado.indisponivelRotulo')}</dt>
          <dd>{t('navegacao.janela.desatualizado.indisponivelTexto')}</dd>
        </div>
        <div>
          <dt>{t('navegacao.janela.desatualizado.fazerRotulo')}</dt>
          <dd>{como}</dd>
        </div>
      </dl>
      {acao ? <div className="aviso-computador-acao">{acao}</div> : null}
      <VStack gap={2} className="aviso-computador-rodape">
        {/* Fora da leitura em voz alta a cada nova checagem: o aviso em si já foi anunciado. */}
        <div aria-live="off" className="aviso-computador-reconecta">
          {aviso.verificando ? <Spinner size={9} /> : null}
          <span>{t(aviso.verificando ? 'navegacao.janela.desatualizado.verificando' : 'navegacao.janela.desatualizado.reconecta')}</span>
        </div>
        <div className="aviso-computador-versoes">
          {doApp
            ? <bdi dir="auto">{t('navegacao.janela.desatualizado.versaoApp', { minimo: aviso.minima })}</bdi>
            : aviso.encontrada
              ? <bdi dir="auto">{t('navegacao.janela.desatualizado.versaoComputador', { versao: aviso.encontrada, minimo: aviso.minima })}</bdi>
              : <bdi dir="auto">{t('navegacao.janela.desatualizado.versaoComputadorIlegivel', { minimo: aviso.minima })}</bdi>}
        </div>
      </VStack>
    </section>
  )
}
