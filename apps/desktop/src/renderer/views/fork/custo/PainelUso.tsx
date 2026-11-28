// DON-908 (C8) + MX-145 E2: a view do painel de Ajustes → Uso, só de props (sem hooks de dados, para o teste de
// render): o mês, o resumo, a aba (Por conta, Por modelo ou Por bot) e os limites do plano. Os totais e a ordem vêm
// prontos do Hub: a UI não soma linhas, não reordena nem converte moedas; sem preço nunca vira zero. Os limites do
// plano (5 h, semana) vêm do `usage` por provedor e aparecem sempre, numa seção própria, em qualquer aba e em
// qualquer estado do relatório: são do provedor, não do ledger, e nunca são ligados a uma conta (o `usage` global
// não prova de qual perfil ele é).
import type { ReactNode } from 'react'
import type { Bot, Usage } from '@shared/models'
import { Button, CardSection, IconButton, Spinner, ValueRow } from '../../../components/Controls'
import { BotAvatar, CharacterAvatar } from '../../../components/Avatar'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n/index'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { isolado, ltr } from '../../../../shared/bidi'
import { FuncaoEtiqueta } from '../../criar-bot/FuncaoEtiqueta'
import { UsageLimits } from '../../settings/UsageLimits'
import { mascotShape, providerTint } from '../../usage/usage-model'
import { dinheiro, mesDe, mesVizinho, nomeDoMes, parcial, rotuloDaConta, type AbaUso, type LinhaConta, type RelatorioCusto, type Totais } from './modelo'
import type { EstadoCusto } from './useCusto'
import type { EstadoCadastro } from './useCadastroContas'
import { parcialDe, tokensDe, valorDe } from './textos'
import { HistoricoCota, type HistoricoProps } from './HistoricoCota'
import './custo.css'

const P = 'conversa.custo'

/** Os limites de um computador (rótulo só quando há mais de um). */
export interface LimitesItem { chave: string; rotulo: ReactNode | null; usage: Usage }

export interface PainelProps {
  estado: EstadoCusto
  lendo: boolean
  online: boolean
  /** O mês mostrado (`YYYY-MM`). */
  mes: string
  onMes: (mes: string) => void
  onAtualizar: () => void
  aba: AbaUso
  cadastro: EstadoCadastro
  bot: (botId: string) => Bot | undefined
  limites: readonly LimitesItem[]
  /** O histórico das cotas (aba Conta); ausente = a seção não existe (quem monta o painel não o liga). */
  historico?: HistoricoProps
}

export function PainelUso(p: PainelProps) {
  const idioma = useIdioma()
  const t = useT()
  const relatorio = p.estado.kind === 'pronto' ? p.estado.relatorio : null
  const rtl = superficieIdioma(idioma).dir === 'rtl'
  const hora = relatorio ? formatarData(idioma, relatorio.measuredAt, { hour: '2-digit', minute: '2-digit' }) : null

  const cabecalho = (
    <HStack gap={6} className="custo-uso-mes">
      <IconButton title={t('conversa.custo.mesAnterior')} icon={rtl ? 'chevron.right' : 'chevron.left'} onClick={() => p.onMes(mesVizinho(p.mes, -1))} disabled={!p.online} />
      <Text tabular className="custo-uso-mes-nome">{nomeDoMes(idioma, p.mes)}</Text>
      <IconButton title={t('conversa.custo.mesSeguinte')} icon={rtl ? 'chevron.left' : 'chevron.right'} onClick={() => p.onMes(mesVizinho(p.mes, 1))} disabled={!p.online || p.mes >= mesDe(Date.now())} />
      <Button kind="secondary" onClick={p.onAtualizar} disabled={!p.online || p.lendo}>{p.lendo ? t('conversa.custo.lendo') : t('conversa.custo.atualizar')}</Button>
    </HStack>
  )

  let corpo
  if (!p.online && !relatorio) corpo = <Text tone="secondary">{t('conversa.custo.semConexao')}</Text>
  else if (p.estado.kind === 'lendo') corpo = <HStack gap={8}><Spinner /><Text tone="secondary">{t('conversa.custo.lendoTexto')}</Text></HStack>
  else if (p.estado.kind === 'falha') {
    corpo = (
      <HStack gap={10} className="custo-uso-mes">
        <Text tone="secondary" grow>{t(`conversa.custo.falha.${p.estado.falha}`)}</Text>
        {p.estado.falha === 'antigo' || p.estado.falha === 'soDono' ? null : <Button kind="secondary" onClick={p.onAtualizar} disabled={p.lendo}>{t('conversa.custo.tentar')}</Button>}
      </HStack>
    )
  } else corpo = <Total relatorio={p.estado.relatorio} />

  // A nota fixa aparece sempre, em todo estado (sem dados do Hub nela). O aviso do passado só com um Hub que tem o custo.
  const compativel = !(p.estado.kind === 'falha' && (p.estado.falha === 'antigo' || p.estado.falha === 'soDono'))
  return (
    <>
      <CardSection title={t('conversa.custo.resumo')} accessory={cabecalho}
        footer={<>{t('conversa.custo.nota')}{compativel ? ` ${t('conversa.custo.retro')}` : ''}{hora ? ` ${t('conversa.custo.medido', { hora })}` : ''}</>}>
        {corpo}
      </CardSection>
      {relatorio && relatorio.total.turns > 0 ? <Aba relatorio={relatorio} aba={p.aba} cadastro={p.cadastro} bot={p.bot} /> : null}
      <LimitesDoPlano itens={p.limites} />
      {p.historico && p.aba === 'conta' ? <HistoricoCota historico={p.historico} cadastro={p.cadastro} /> : null}
    </>
  )
}

/** "Entrada 3,4 mi · Saída 1,7 mi · Parcial: 2 de 9 turnos sem preço", com o que vem antes (turnos). */
function detalhe(t: ReturnType<typeof useT>, idioma: ReturnType<typeof useIdioma>, totais: Totais, antes: string[] = []) {
  return [...antes, tokensDe(t, idioma, P, totais), parcialDe(t, idioma, P, totais)].filter(Boolean).join(' · ')
}

function Total({ relatorio }: { relatorio: RelatorioCusto }) {
  const idioma = useIdioma()
  const t = useT()
  const total = relatorio.total
  if (total.turns === 0) return <VStack gap={4}><Text tone="secondary">{t('conversa.custo.vazio')}</Text><Text tone="secondary" size="callout">{t('conversa.custo.vazioDica')}</Text></VStack>
  const turnos = t('conversa.custo.turnos', { n: formatarNumero(idioma, total.turns) })
  return (
    <ValueRow label={t('conversa.custo.total', { mes: nomeDoMes(idioma, relatorio.month) })} detail={detalhe(t, idioma, total, [turnos])}>
      <Valor totais={total} />
    </ValueRow>
  )
}

/**
 * Uma linha com rosto e selo ao lado do nome (as telas 01 e 03); sem rosto nem selo, a mesma de `ValueRow`. O detalhe
 * (tokens, turnos) fica sob o nome (tela 03, bot) ou, com `sobOValor`, sob o dinheiro à direita (tela 01, conta).
 */
function Linha({ rosto, nome, selo, detail, sobOValor, children }: { rosto?: ReactNode; nome: string; selo?: ReactNode; detail: string; sobOValor?: boolean; children: ReactNode }) {
  const nota = <span className="detail-note" title={detail}>{detail}</span>
  return (
    <div className="value-row">
      <HStack gap={10} grow shrink>
        {rosto}
        <VStack as="span" gap={3} grow shrink>
          <HStack gap={6}>
            <Text tone="text" truncate>{nome}</Text>
            {selo}
          </HStack>
          {sobOValor ? null : nota}
        </VStack>
      </HStack>
      <Spacer min={12} />
      {sobOValor
        ? <VStack as="span" gap={2} align="flex-end" className="value-row-value">{children}{nota}</VStack>
        : <HStack as="span" className="value-row-value">{children}</HStack>}
    </div>
  )
}

function Aba({ relatorio, aba, cadastro, bot }: { relatorio: RelatorioCusto; aba: AbaUso; cadastro: EstadoCadastro; bot: PainelProps['bot'] }) {
  const idioma = useIdioma()
  const t = useT()
  const turnos = (totais: Totais) => t('conversa.custo.turnos', { n: formatarNumero(idioma, totais.turns) })

  if (aba === 'bot') {
    if (!relatorio.bots.length) return null
    return (
      <CardSection title={t('conversa.custo.porBot')} footer={t('conversa.custo.porBotNota')}>
        {relatorio.bots.map((b) => {
          const cadastrado = bot(b.botId)
          return (
            <Linha key={b.botId} rosto={cadastrado ? <BotAvatar bot={cadastrado} size={24} animated={false} /> : undefined} nome={isolado(b.name || b.botId)} selo={cadastrado ? <FuncaoEtiqueta bot={cadastrado} /> : undefined} detail={detalhe(t, idioma, b.totals, [turnos(b.totals)])}>
              <Valor totais={b.totals} />
            </Linha>
          )
        })}
      </CardSection>
    )
  }

  const titulo = t(aba === 'conta' ? 'conversa.custo.contasIa' : 'conversa.custo.porModelo')
  // Hub antigo: o relatório vale (total e bots), mas não manda os grupos por conta e por modelo.
  if (relatorio.contas === null || relatorio.modelos === null) {
    return <CardSection title={titulo}><Text tone="secondary">{t('conversa.custo.antigoGrupos')}</Text></CardSection>
  }
  if (aba === 'modelo') {
    return (
      <CardSection title={titulo}>
        {relatorio.modelos.map((m) => (
          <ValueRow key={m.model ?? 'sem-modelo'} label={m.model === null ? t('conversa.custo.modeloNaoIdentificado') : ltr(m.model)} detail={detalhe(t, idioma, m.totals, [turnos(m.totals)])}><Valor totais={m.totals} /></ValueRow>
        ))}
      </CardSection>
    )
  }
  return <PorConta titulo={titulo} linhas={relatorio.contas} cadastro={cadastro} />
}

function PorConta({ titulo, linhas, cadastro }: { titulo: string; linhas: LinhaConta[]; cadastro: EstadoCadastro }) {
  const idioma = useIdioma()
  const t = useT()
  const contas = cadastro.kind === 'pronto' ? cadastro.contas : null
  return (
    <CardSection title={titulo} footer={cadastro.kind === 'falha' ? t('conversa.custo.contasSemNomes') : null}>
      {linhas.map((c) => {
        const rotulo = rotuloDaConta(c.accountId, contas)
        const nome = rotulo.kind === 'naoIdentificada' ? t('conversa.custo.contaNaoIdentificada') : rotulo.kind === 'curto' ? t('conversa.custo.contaCurta', { id: ltr(rotulo.id) }) : isolado(rotulo.label)
        const tipo = rotulo.kind === 'nome' && rotulo.tipo ? t(`conversa.custo.tipo.${rotulo.tipo}`) : null
        const rosto = rotulo.kind === 'nome' ? <CharacterAvatar shape={mascotShape(rotulo.backend)} color="" tint={providerTint(rotulo.backend)} size={24} mood="idle" /> : undefined
        return (
          <Linha key={c.accountId ?? 'sem-conta'} sobOValor rosto={rosto} nome={nome} selo={tipo ? <FuncaoEtiqueta bot={{ kind: 'bot', title: tipo }} /> : undefined}
            detail={detalhe(t, idioma, c.totals, [t('conversa.custo.turnos', { n: formatarNumero(idioma, c.totals.turns) })])}>
            <Valor totais={c.totals} />
          </Linha>
        )
      })}
    </CardSection>
  )
}

/**
 * Os limites do plano por provedor, de todos os computadores (como a lista de antes). Sempre presentes: sem nenhum
 * limite conhecido, a seção diz isso. O `usage` é o último conhecido (offline não o apaga).
 */
function LimitesDoPlano({ itens }: { itens: readonly LimitesItem[] }) {
  const t = useT()
  const comLimites = itens.filter((i) => i.usage.providers.length > 0)
  return (
    <CardSection title={t('conversa.custo.limites')} footer={t('conversa.custo.limitesNota')}>
      {comLimites.length === 0 ? <Text tone="secondary">{t('ajustes.uso.vazio')}</Text> : comLimites.map((i) => (
        <VStack key={i.chave} gap={10} align="stretch">
          {i.rotulo}
          <UsageLimits usage={i.usage} />
        </VStack>
      ))}
    </CardSection>
  )
}

/** O dinheiro informado pelo motor, com "(parcial)" junto quando turnos ficaram sem preço; sem preço, "Sem preço" esmaecido. */
function Valor({ totais }: { totais: Totais }) {
  const idioma = useIdioma()
  const t = useT()
  const estimado = dinheiro(idioma, totais.estimatedCosts ?? [])
  return (
    <VStack as="span" gap={2} align="flex-end">
      {totais.costs.length || !estimado ? <HStack gap={6}>
        <Text tone={totais.costs.length ? 'text' : 'secondary'} tabular>{valorDe(t, idioma, totais)}</Text>
        {totais.costs.length > 0 && parcial(totais) ? <Text tone="attention" tabular>{t('conversa.custo.parcialCurto')}</Text> : null}
      </HStack> : null}
      {estimado ? <Text tabular>{t('conversa.custo.estimado', { valor: estimado })}</Text> : null}
      {!totais.costs.length && parcial(totais) ? <Text tone="attention" tabular>{t('conversa.custo.parcialCurto')}</Text> : null}
    </VStack>
  )
}
