// Tela 05: "Contas que o bot usa". Uma tabela de leitura única, uma linha por serviço: Serviço · Contas · Se o pedido
// não diz a conta · Ações perigosas. Os detalhes (marcar contas, a conta padrão) abrem ao clicar na linha.
// A coluna de ações perigosas só existe quando alguma ficha tem alguma, e hoje só lista (desligadas): as chaves entram
// com o filtro da fatia 2.
import { useCallback, useEffect, useState } from 'react'
import type { Bot } from '@shared/models'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { extensoesScreen } from '../extensoes/lugar-atual'
import { definirNoBot, lerDoBot, mercadoPara, useMercado } from './estado'
import { linhasDoBot, nomeCurto, temAcoesPerigosas, type LinhaDoBot } from './regras.ts'
import type { PedidoDefinirVinculo, VinculosDoBot } from './tipos.ts'
import { Aviso, Carregando, Erro, frasesDaRecusa, LogoServico, Titulo } from './pecas'

export function ContasDoBot({ bot }: { bot: Pick<Bot, 'id' | 'name'> }) {
  const store = useStore()
  const t = useT()
  const mercado = useMercado(store)
  const principal = useModel(store.principal)
  const ehPrincipal = principal.id === bot.id
  const [vinculos, setVinculos] = useState<VinculosDoBot | null>(null)
  const [falhou, setFalhou] = useState(false)
  const [erro, setErro] = useState<string | null>(null)
  const [aberta, setAberta] = useState<string | null>(null)
  const [salvando, setSalvando] = useState(false)
  const [escolher, setEscolher] = useState(false)
  const online = store.connection.kind === 'online'

  const hub = store.hello?.hostId ?? null
  const ler = useCallback(async () => {
    setFalhou(false)
    try { setVinculos(await lerDoBot(store, bot.id)) } catch { setFalhou(true) }
  }, [store, bot.id])
  useEffect(() => { setVinculos(null); setAberta(null); setEscolher(false); if (online && !ehPrincipal) { void ler(); void mercadoPara(store).ler() } }, [bot.id, hub, online, ehPrincipal, ler, store])

  const definir = async (pedido: Omit<PedidoDefinirVinculo, 'botId'>) => {
    setSalvando(true); setErro(null)
    try { setVinculos(await definirNoBot(store, { botId: bot.id, ...pedido })) } catch (e) { setErro(frasesDaRecusa(t, e)); await ler() } finally { setSalvando(false) }
  }

  if (!online) return null
  // O principal do Space nunca recebe conector (regra do Hub): sem tabela nem leitura de contas, só a explicação.
  if (ehPrincipal) {
    return <section className="mercado-secao mercado-no-bot"><Titulo titulo={t('mercado.bot.titulo', { bot: bot.name })} sub={t('mercado.bot.sub')} nivel="title3" /><Aviso>{t('mercado.bots.berga')}</Aviso></section>
  }
  const catalogo = mercado.catalogo
  const titulo = <Titulo titulo={t('mercado.bot.titulo', { bot: bot.name })} sub={t('mercado.bot.sub')} nivel="title3" />
  const verContas = (
    <button className="mercado-link" onClick={() => { mercadoPara(store).abrirAba('contas'); extensoesScreen.show('connectors', store.computer.id) }}>
      <HStack gap={4}><Text size="callout" weight="semibold">{t('mercado.bot.minhas')}</Text><Icon name="chevron.right" size={10} /></HStack>
    </button>
  )

  if (falhou || mercado.estado === 'erro' || mercado.contasErro !== null) {
    return <section className="mercado-secao mercado-no-bot"><div className="mercado-secao-topo">{titulo}{verContas}</div><Erro>{t('mercado.bot.erro')}</Erro><div><Button kind="secondary" onClick={() => { void ler(); void mercado.ler() }}>{t('mercado.catalogo.tentar')}</Button></div></section>
  }
  if (!vinculos || mercado.estado !== 'lida') return <section className="mercado-secao mercado-no-bot">{titulo}<Carregando texto={t('mercado.bot.carregando')} linhas={2} /></section>

  const linhas = linhasDoBot(catalogo, mercado.contas, vinculos)
  const nenhumaLigada = linhas.every((l) => l.ligadas.length === 0)
  const todasAsContas = vinculos.contas.length
  const comPerigosas = temAcoesPerigosas(linhas)

  return (
    <section className="mercado-secao mercado-no-bot" aria-label={t('mercado.bot.titulo', { bot: bot.name })}>
      <div className="mercado-secao-topo">{titulo}{verContas}</div>
      {erro ? <Erro>{erro}</Erro> : null}
      {linhas.length === 0 ? (
        <Aviso>{t('mercado.bot.vazio.semContas')}</Aviso>
      ) : nenhumaLigada && !escolher ? (
        <VStack gap={8} className="mercado-vazio" align="center">
          <Text size="compactBody" weight="semibold" tone="text">{t('mercado.bot.vazio.titulo', { bot: bot.name })}</Text>
          <Text size="callout" tone="secondary">{t('mercado.bot.vazio.texto', { n: todasAsContas })}</Text>
          <Button kind="secondary" onClick={() => { setEscolher(true); setAberta(linhas[0].servico) }}>{t('mercado.bot.vazio.escolher')}</Button>
        </VStack>
      ) : (
        <div className={`mercado-tabela${comPerigosas ? ' com-perigosas' : ''}`}>
          <div className="mercado-tabela-cab" aria-hidden>
            <Text size="callout" weight="semibold" as="div">{t('mercado.bot.col.servico')}</Text>
            <Text size="callout" weight="semibold" as="div">{t('mercado.bot.col.contas')}</Text>
            <Text size="callout" weight="semibold" as="div">{t('mercado.bot.col.pedido')}</Text>
            {comPerigosas ? <Text size="callout" weight="semibold" as="div">{t('mercado.bot.col.perigosas')}</Text> : null}
            <span />
          </div>
          {linhas.map((l) => (
            <Linha key={l.servico} linha={l} aberta={aberta === l.servico} comPerigosas={comPerigosas} salvando={salvando}
              alternar={() => setAberta(aberta === l.servico ? null : l.servico)} definir={definir} />
          ))}
        </div>
      )}
      <HStack gap={8}><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('mercado.bot.rodape')}</Text></HStack>
    </section>
  )
}

function Linha({ linha, aberta, comPerigosas, salvando, alternar, definir }: {
  linha: LinhaDoBot; aberta: boolean; comPerigosas: boolean; salvando: boolean
  alternar: () => void; definir: (p: Omit<PedidoDefinirVinculo, 'botId'>) => Promise<void>
}) {
  const t = useT()
  const curto = (nome: string) => nomeCurto(nome, linha.nome)
  const ligadas = linha.ligadas
  const pedido = linha.seODizNaoPedido
  const contasTexto = ligadas.length === 0
    ? <Text size="body" tone="secondary">{t('mercado.bot.naoUsa')}</Text>
    : <Text size="body" tone="text">{ligadas.map((c, i) => (
        <span key={c.id} className={c.expirada ? 'mercado-ambar' : undefined}>{i > 0 ? (i === ligadas.length - 1 ? ` ${t('mercado.bot.e')} ` : ', ') : ''}{c.expirada ? t('mercado.bot.expirou', { nome: curto(c.nome) }) : curto(c.nome)}</span>
      ))}</Text>
  const seDiz = pedido.tipo === 'nenhuma' ? '—' : pedido.tipo === 'pergunta' ? t('mercado.bot.pergunta') : t('mercado.bot.usa', { conta: curto(linha.contas.find((c) => c.id === pedido.conta)?.nome ?? linha.nome) })
  const idDoDetalhe = `mercado-bot-${linha.servico}`
  return (
    <>
      <button className="mercado-linha-bot" aria-expanded={aberta} aria-controls={idDoDetalhe} aria-label={aberta ? t('mercado.bot.fechar', { servico: linha.nome }) : t('mercado.bot.abrir', { servico: linha.nome })} onClick={alternar}>
        <HStack gap={10} shrink><LogoServico nome={linha.nome} marca={linha.marca} size={28} /><Text size="body" weight="semibold" tone="text" truncate>{linha.nome}</Text></HStack>
        {contasTexto}
        <Text size="body" tone={pedido.tipo === 'nenhuma' ? 'tertiary' : 'text'}>{seDiz}</Text>
        {comPerigosas ? <Text size="body" tone={linha.perigosas.length ? 'text' : 'tertiary'}>{linha.perigosas.length ? t('mercado.perigosas.desligadas') : '—'}</Text> : null}
        <span className="mercado-chevron"><Icon name="chevron.right" size={11} /></span>
      </button>
      {aberta ? (
        <div id={idDoDetalhe} className="mercado-detalhe-bot" role="group" aria-label={linha.nome}>
          <Text size="caption" tone="secondary">{t('mercado.bot.detalhe.nota')}</Text>
          <fieldset className="mercado-opcao-coluna" disabled={salvando}>
            {linha.contas.map((c) => (
              <label key={c.id} className="mercado-opcao">
                <input type="checkbox" checked={c.ligada} onChange={() => void definir(c.ligada ? { desligar: [c.id] } : { ligar: [c.id] })} />
                <VStack gap={0} shrink>
                  <Text size="body" tone="text">{c.expirada ? t('mercado.bot.expirou', { nome: c.nome }) : c.nome}</Text>
                  {c.identidade ? <Text size="caption" tone="secondary">{c.identidade}</Text> : null}
                </VStack>
              </label>
            ))}
          </fieldset>
          <fieldset className="mercado-opcao-coluna" disabled={salvando || ligadas.length === 0}>
            {ligadas.map((c) => (
              <label key={c.id} className="mercado-opcao">
                <input type="radio" name={`${idDoDetalhe}-padrao`} checked={pedido.tipo === 'usa' && pedido.conta === c.id} onChange={() => void definir({ padrao: { [linha.servico]: c.id }, perguntar: { [linha.servico]: false } })} />
                <Text size="body" tone="text">{t('mercado.bot.usar', { conta: curto(c.nome) })}</Text>
              </label>
            ))}
            {ligadas.length > 1 ? (
              <label className="mercado-opcao">
                <input type="radio" name={`${idDoDetalhe}-padrao`} checked={pedido.tipo === 'pergunta'} onChange={() => void definir({ perguntar: { [linha.servico]: true } })} />
                <Text size="body" tone="text">{t('mercado.bot.perguntar')}</Text>
              </label>
            ) : null}
          </fieldset>
          {comPerigosas ? (
            <div className="mercado-opcao-coluna">
              {linha.perigosas.map((p) => (
                <HStack key={p.ferramenta} gap={8}><Text size="body" tone="text" grow>{p.rotulo}</Text><Text size="callout" tone="secondary">{t('mercado.perigosas.desligadas')}</Text></HStack>
              ))}
              {linha.perigosas.length ? <Text size="caption" tone="secondary">{t('mercado.bot.perigosas.nota')}</Text> : null}
            </div>
          ) : null}
        </div>
      ) : null}
    </>
  )
}
