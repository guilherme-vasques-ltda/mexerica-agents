// Histórico das COTAS do plano (`donnaUsageHistory`), dentro da aba Conta de Ajustes → Uso. Só props (sem hooks de dados,
// para o teste de render). Não mistura grandezas: aqui é só percentual usado de cada janela, por conta; tokens e
// dinheiro ficam nas seções do relatório de custo. Sem leitura nunca vira 0%: a janela diz "Sem leituras".
import { Button, CardSection, ValueRow } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n/index'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { isolado, ltr } from '../../../../shared/bidi'
import { FAIXAS_HISTORICO, porConta, type FaixaHistorico, type JanelaHistorico } from './historico'
import { rotuloDaConta } from './modelo'
import type { EstadoCadastro } from './useCadastroContas'
import type { EstadoHistorico } from './useHistorico'

const P = 'conversa.custo.historico'

export interface HistoricoProps {
  estado: EstadoHistorico
  lendo: boolean
  online: boolean
  faixa: FaixaHistorico
  onFaixa: (faixa: FaixaHistorico) => void
  onAtualizar: () => void
}

export function HistoricoCota({ historico, cadastro }: { historico: HistoricoProps; cadastro: EstadoCadastro }) {
  const t = useT()
  const h = historico
  const faixas = (
    <HStack gap={6} className="custo-uso-mes">
      {FAIXAS_HISTORICO.map((f) => <Button key={f} kind={f === h.faixa ? 'primary' : 'secondary'} onClick={() => h.onFaixa(f)} disabled={!h.online}>{t(`${P}.faixa.${f}`)}</Button>)}
      <Button kind="secondary" onClick={h.onAtualizar} disabled={!h.online || h.lendo}>{h.lendo ? t('conversa.custo.lendo') : t('conversa.custo.atualizar')}</Button>
    </HStack>
  )
  let corpo
  if (h.estado.kind === 'pronto') corpo = <Janelas historico={h.estado.historico} cadastro={cadastro} />
  else if (!h.online) corpo = <Text tone="secondary">{t('conversa.custo.semConexao')}</Text>
  else if (h.estado.kind === 'lendo') corpo = <Text tone="secondary">{t('conversa.custo.lendo')}</Text>
  else corpo = <Text tone="secondary">{t(`${P}.falha.${h.estado.falha}`)}</Text>
  return <CardSection title={t(`${P}.titulo`)} accessory={faixas} footer={t(`${P}.nota`)}>{corpo}</CardSection>
}

function Janelas({ historico, cadastro }: { historico: Extract<EstadoHistorico, { kind: 'pronto' }>['historico']; cadastro: EstadoCadastro }) {
  const t = useT()
  const contas = cadastro.kind === 'pronto' ? cadastro.contas : null
  if (historico.windows.length === 0) {
    return <VStack gap={4}><Text tone="secondary">{t(`${P}.vazio`)}</Text><Text tone="secondary" size="callout">{t(`${P}.vazioDica`)}</Text></VStack>
  }
  return (
    <VStack gap={12} align="stretch">
      {porConta(historico.windows).map((grupo) => {
        const rotulo = rotuloDaConta(grupo.accountId, contas)
        const nome = rotulo.kind === 'naoIdentificada' ? t('conversa.custo.contaNaoIdentificada') : rotulo.kind === 'curto' ? t('conversa.custo.contaCurta', { id: ltr(rotulo.id) }) : isolado(rotulo.label)
        return (
          <VStack key={grupo.accountId ?? 'sem-conta'} gap={4} align="stretch">
            <Text tone="secondary" size="callout">{nome}</Text>
            {grupo.janelas.map((j) => <Janela key={`${j.provider}/${j.windowId}`} janela={j} bucketMs={historico.bucketMs} />)}
          </VStack>
        )
      })}
      {historico.truncated ? <Text tone="attention" size="callout">{t(`${P}.truncado`)}</Text> : null}
    </VStack>
  )
}

function Janela({ janela, bucketMs }: { janela: JanelaHistorico; bucketMs: number }) {
  const idioma = useIdioma()
  const t = useT()
  const quando = (ms: number) => formatarData(idioma, ms, { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })
  const primeiro = janela.buckets[0]
  const ultimo = janela.buckets.at(-1)
  const faixa = primeiro && ultimo ? ultimo.start - primeiro.start + bucketMs : bucketMs
  const pct = (n: number) => `${formatarNumero(idioma, n, { maximumFractionDigits: 0 })}%`
  const previsao = janela.forecast === null ? t(`${P}.previsao.indisponivel`)
    : janela.forecast.exhausted ? (janela.forecast.exhaustsAt === null ? t(`${P}.previsao.esgotadaSemHora`) : t(`${P}.previsao.esgotada`, { quando: quando(janela.forecast.exhaustsAt) }))
    : janela.forecast.exhaustsAt === null ? t(`${P}.previsao.nenhuma`)
    : t(`${P}.previsao.estimada`, { quando: quando(janela.forecast.exhaustsAt) })
  const reinicio = janela.resetsAt === null ? t(`${P}.semReinicio`) : t(`${P}.reinicia`, { quando: quando(janela.resetsAt) })
  const detalhe = ultimo ? [reinicio, previsao].join(' · ') : ''
  const rotulo = janela.windowId === 'primary' || janela.windowId === 'secondary' ? t(`${P}.janela.${janela.windowId}`)
    : janela.windowId === 'session' ? t('sistema.barra.uso.sessao')
    : janela.windowId === 'week' ? t('sistema.barra.uso.semanal') : ltr(janela.windowId)
  return (
    <ValueRow label={`${ltr(janela.provider)} · ${rotulo}`} detail={detalhe}>
      {primeiro && ultimo ? (
        <VStack as="span" gap={4} align="flex-end">
          <Text tabular>{t(`${P}.ultimo`, { pct: pct(ultimo.percent) })}</Text>
          <span className="custo-historico-barras" role="img" aria-label={t(`${P}.barras`)}>
            {janela.buckets.map((b) => <span key={b.start} className="custo-historico-barra" style={{
              insetInlineStart: `${100 * (b.start - primeiro.start) / faixa}%`,
              inlineSize: `${80 * bucketMs / faixa}%`,
              height: `${Math.min(100, b.percent)}%`,
            }} title={`${quando(b.start)} · ${pct(b.percent)}`} />)}
          </span>
        </VStack>
      ) : <Text tone="secondary">{t(`${P}.semLeituras`)}</Text>}
    </ValueRow>
  )
}
