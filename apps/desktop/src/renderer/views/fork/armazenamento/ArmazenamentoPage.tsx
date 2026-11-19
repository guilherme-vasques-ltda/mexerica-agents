import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { formatarNumero } from '../../../lib/fork-i18n/index'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Button, CardForm, CardSection, ProgressBar, Spinner, ValueRow } from '../../../components/Controls'
import { HStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { ComputerBadge } from '../../ComputerBadge'
import { bancoUsado, detalheConversas, fracoes, horaMedida, lerRelatorio, ordenarBots, tamanho, textoDaFalha, type BotArmazenamento, type RelatorioArmazenamento } from './modelo'
import './armazenamento.css'

// DON-905 (C6, docs/reference/design/don-905-c6/README.md): Ajustes → Armazenamento, só leitura. O Hub selecionado (senão o
// deste computador), como Plugins: o Hub decide quem pode ler (a mesma autoridade de dono do WhatsApp, inclusive um
// aparelho com Control) e recusa os outros; a UI não restringe a loopback. Lido ao abrir e por Atualizar; nada vai
// para o mirror. Limpar por idade fica para a C7. Contrato e limitações: docs/features/armazenamento.md. O WhatsApp é
// só texto: a main ainda não tem o ajuste de prazo (PR #414), então nada de link para ele.

export function ArmazenamentoPage() {
  const t = useT()
  const idioma = useIdioma()
  const app = useApp()
  const store = app.selectedStore ?? app.local
  return (
    <div {...superficieIdioma(idioma)} className="idioma-superficie" style={{ display: 'contents' }}><CardForm>
      {store && app.computers.length > 1 ? (
        <HStack gap={6} className="settings-usage-computer">
          <ComputerBadge computer={store.computer} size={16} />
          {store.hostName}
        </HStack>
      ) : null}
      {store ? <Relatorio key={store.computer.id} source={store} /> : <Text tone="secondary">{t('armazenamento.semConexao')}</Text>}
    </CardForm></div>
  )
}


type Estado = { kind: 'medindo' } | { kind: 'pronto'; relatorio: RelatorioArmazenamento } | { kind: 'falha'; erro: unknown }

function Relatorio({ source }: { source: BotStore }) {
  const t = useT()
  const idioma = useIdioma()
  const store = useModel(source)
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const origem = `${store.hello?.hostId ?? ''}|${store.principalEpoch}`
  const [estado, setEstado] = useState<Estado>({ kind: 'medindo' })
  const [medindo, setMedindo] = useState(false)
  const leitura = useRef(0)

  const medir = useCallback(() => {
    if (!client) return
    const minha = ++leitura.current
    setMedindo(true)
    client.call('donnaStorageReport', {}, 60_000).then(lerRelatorio).then(
      (relatorio) => { if (leitura.current === minha) setEstado({ kind: 'pronto', relatorio }) },
      (e: unknown) => { if (leitura.current === minha) setEstado({ kind: 'falha', erro: e }) },
    ).finally(() => { if (leitura.current === minha) setMedindo(false) })
  }, [client])

  // Outro Hub ou outro principal: começa do zero. Abrir a página ou voltar a conexão mede de novo.
  useEffect(() => { leitura.current++; setEstado({ kind: 'medindo' }); setMedindo(false) }, [origem])
  useEffect(() => { medir(); return () => { leitura.current++ } }, [medir, origem])

  if (estado.kind !== 'pronto') {
    if (!client) return <Text tone="secondary">{t('armazenamento.semConexao')}</Text>
    if (estado.kind === 'medindo') return <HStack gap={8}><Spinner /><Text tone="secondary">{t('armazenamento.medindo')}</Text></HStack>
    return (
      <HStack gap={10} className="armazenamento-falha">
        <Text tone="secondary" grow>{textoDaFalha(estado.erro, idioma)}</Text>
        {textoDaFalha(estado.erro, idioma) === t('armazenamento.hubAntigo') || textoDaFalha(estado.erro, idioma) === t('armazenamento.soDono') ? null : <Button kind="secondary" onClick={medir} disabled={medindo}>{t('armazenamento.tentarDeNovo')}</Button>}
      </HStack>
    )
  }

  const { relatorio } = estado
  const total = relatorio.total
  return (
    <>
      <CardSection title={t('armazenamento.totalTitulo')} accessory={
        <HStack gap={10}>
          <Text size="caption" tone="secondary">{client ? t('armazenamento.medidoAs', { hora: horaMedida(relatorio.measuredAt, undefined, idioma) }) : t('armazenamento.semConexao')}</Text>
          <Button kind="secondary" onClick={medir} disabled={!client || medindo}>{medindo ? t('armazenamento.atualizando') : t('armazenamento.atualizar')}</Button>
        </HStack>
      }>
        <ValueRow label={t('armazenamento.banco')} detail={t('armazenamento.bancoDetalhe', { arquivo: tamanho(total.databaseBytes, idioma), wal: tamanho(total.walBytes, idioma) })}><Valor bytes={bancoUsado(total)} /></ValueRow>
        <ValueRow label={t('armazenamento.livreBanco')} detail={t('armazenamento.livreBancoDetalhe')}><Valor bytes={total.databaseFreeBytes} /></ValueRow>
        <ValueRow label={t('armazenamento.livreDisco')}><Valor bytes={total.diskFreeBytes} /></ValueRow>
      </CardSection>
      {relatorio.bots === null ? (
        <CardSection title={t('armazenamento.botsTitulo')}><Text tone="secondary" className="armazenamento-nota">{t('armazenamento.botsIndisponivel')}</Text></CardSection>
      ) : relatorio.bots.length === 0 ? (
        <CardSection title={t('armazenamento.botsTitulo')}><Text tone="secondary" className="armazenamento-nota">{t('armazenamento.botsVazio')}</Text></CardSection>
      ) : (
        <Bots bots={relatorio.bots} />
      )}
    </>
  )
}

function Bots({ bots }: { bots: BotArmazenamento[] }) {
  const t = useT()
  const idioma = useIdioma()
  const fracao = fracoes(bots)
  return (
    <>
      <Text as="div" size="footnote" tone="secondary" className="armazenamento-nota">{t('armazenamento.botsNota')}</Text>
      {ordenarBots(bots, idioma).map((bot) => {
        const f = fracao.get(bot.id) ?? null
        return (
          <CardSection key={bot.id} title={bot.name} accessory={<Text tone="text" weight="semibold" tabular>{tamanho(bot.bytes, idioma)}</Text>}>
            {f === null ? null : <div className="armazenamento-barra"><ProgressBar value={f} label={t('armazenamento.botParte', { nome: bot.name, pct: formatarNumero(idioma, Math.round(f * 100)) })} /></div>}
            <ValueRow label={t('armazenamento.conversaPrincipal')} detail={detalheConversas(bot.messages.main, undefined, idioma)}><Valor bytes={bot.messages.main?.bytes ?? null} /></ValueRow>
            <ValueRow label={t('armazenamento.outrasConversas')} detail={detalheConversas(bot.messages.extras, undefined, idioma)}><Valor bytes={bot.messages.extras?.bytes ?? null} /></ValueRow>
            <ValueRow label={t('armazenamento.anexos')}><Valor bytes={bot.attachmentsBytes} /></ValueRow>
            <ValueRow label={t('armazenamento.memoria')} detail={t('armazenamento.memoriaDetalhe', { registros: tamanho(bot.memory.rowsBytes, idioma), arquivos: tamanho(bot.memory.filesBytes, idioma) })}><Valor bytes={bot.memory.bytes} /></ValueRow>
            <ValueRow label={t('armazenamento.whatsapp')} detail={t('armazenamento.whatsappDetalhe', { hub: tamanho(bot.whatsapp.hubBytes, idioma), wacli: tamanho(bot.whatsapp.wacliBytes, idioma) })}><Valor bytes={bot.whatsapp.bytes} /></ValueRow>
          </CardSection>
        )
      })}
      <Text as="div" size="footnote" tone="secondary" className="armazenamento-nota">{t('armazenamento.whatsappPrazo')}</Text>
    </>
  )
}

/** Um tamanho; indisponível fica visível e esmaecido, nunca 0. */
function Valor({ bytes }: { bytes: number | null }) {
  const idioma = useIdioma()
  return <Text tone={bytes === null ? 'secondary' : 'text'} tabular>{tamanho(bytes, idioma)}</Text>
}
