import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Button, CardForm, CardSection, Disclosure, Spinner, ValueRow } from '../../../components/Controls'
import { Dialog, ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import type { CatalogSkill, InstalledSkillV2, SkillDetailsV2, SkillReceipt } from '../../../store/fork-skills'
import type { CatalogoStore, InspecaoSkills, IntencaoHub } from './catalogo-store'
import { useModel } from '../../../lib/observable'
import { ResumoInformativo } from './ResumoInformativo'
import { estadoDaConferencia, leituraVigente, resumoDaInstalada } from './resumo-informativo'
import { AcaoInstalar, SelosDoItem } from './partes'
import { capturarRemocao, codigoDeSkill, curto, detalhesDaVersao, estadoDoItem, fichaDaInstalada, instaladaDoItem, mensagemDeErro, rotuloDisponibilidade, tamanho, versaoDaFolha, type AlvoRemocao } from './regras'
import { TextoPassivo } from './TextoPassivo'
import { AvisoDeRemocao, ConferenciaSkill, UsoDaSkill } from './Acabamento'
import { useStore } from '../../../store/context'
import { ComparaRascunho, type RascunhoRetido, type RetencaoRascunho, type SessaoRevisao } from './ComparaRascunho'
import { destinoDoRetido } from './revisao'
import { textoDe, frase, frases, juntas, type Frase, type SufixoSemParams } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'

/** Rótulo da folha de detalhes (`skills.detalhes.*`) no idioma da tela. */
function useRotulos() {
  const idioma = useIdioma()
  return { idioma, tx: (f: Frase) => textoDe(f, idioma), L: (chave: SufixoSemParams<'skills.detalhes.'>) => textoDe(frase(`skills.detalhes.${chave}`), idioma) }
}
import { traduzir } from '../../../lib/fork-i18n/index.ts'

/** Uma resposta de `skillDetails`, presa à consulta (versão e tentativa) em que foi pedida. */
// Lição da R1 do #572: o erro da leitura fica como frase e é traduzido ao mostrar.
type Leitura = { consulta: string; detalhes: SkillDetailsV2 | null; erro: Frase | null; codigo: string | null }
/** Conteúdo da instalação que o Hub não confere ou não lê (`contentStatus`): estado verdadeiro, sem texto. */
type ConteudoRecusado = 'changed' | 'unavailable'

/**
 * Os detalhes de uma versão só. Pelo catálogo (`item`), a versão atual: ação, motivo, perdas,
 * requisitos, execução desligada, origem, recibo e o texto passivo. Por uma instalada
 * (`instalada`), o registro e o recibo dela; a versão de agora do catálogo (`versaoAtual`) só num
 * cartão à parte. Tudo do Hub e da conexão em que a folha abriu.
 *
 * A folha acompanha a linha de onde abriu: se a mesma ID passa a ser outra encarnação (ou o item
 * muda de digest), ela passa inteira para a versão de agora. Texto e metadados só aparecem com a
 * resposta pedida para a versão mostrada, e essa resposta precisa ser dela (D166 R5/R8). Conteúdo
 * alterado ou indisponível no Hub não mostra texto; remover segue, e a curada pode ser reinstalada (D306).
 */
export function DetalhesSkill({ loja, item, instalada, versaoAtual = null, inspecao, rascunho }: {
  inspecao?: InspecaoSkills
  /** d2a/D2A-UI1: memória do texto da comparação desta abertura; sem ela, a folha não oferece comparar. */
  rascunho?: RetencaoRascunho
  loja: CatalogoStore
  item: CatalogSkill | null
  instalada: InstalledSkillV2 | null
  /** Só com `instalada`: o item do catálogo de agora com o mesmo `catalogId` do recibo. */
  versaoAtual?: CatalogSkill | null
}) {
  // FormSheet conserva este filho na saída; ele precisa observar a troca por conta própria.
  useModel(loja)
  // A folha continua montada na saída do portal: assinar também a conexão, não só a loja.
  const store = useStore()
  const [origemDaFolha] = useState(() => inspecao?.origem ?? loja.capturarIntencao())
  const versao = versaoDaFolha(item, instalada)
  /** A versão que estava na linha quando a folha abriu: se a de agora for outra, a folha avisa. */
  const [versaoAberta] = useState(versao)
  const [leitura, setLeitura] = useState<Leitura | null>(null)
  const [tentativa, setTentativa] = useState(0)
  const [confirmar, setConfirmar] = useState<{ alvo: AlvoRemocao; origem: IntencaoHub } | null>(null)
  const [erroReleitura, setErroReleitura] = useState<Frase | null>(null)
  const textos = useTextosSkills()
  const { idioma, tx, L } = useRotulos()
  const [reselecionar, setReselecionar] = useState(false)
  const [arquivos, setArquivos] = useState(false)
  /**
   * D2A-UI1: a folha recomeçou (queda/hello) com um rascunho desta abertura. Só o mesmo Hub e a mesma
   * encarnação voltam à comparação, e só com o texto; outro Hub ou outra instalação não o adotam.
   */
  const [retido] = useState<{ r: RascunhoRetido; adota: boolean; destino: 'adota' | 'outroHub' | 'outraInstalacao' } | null>(() => {
    const r = !inspecao ? rascunho?.ler() ?? null : null
    const destino = destinoDoRetido(r, instalada, origemDaFolha?.hub ?? null)
    if (!r || !destino) return null
    if (destino !== 'adota') rascunho?.guardar(null)
    return { r, adota: destino === 'adota', destino }
  })
  /** O texto retido vale uma vez, para a comparação que volta; depois de Voltar, o clique começa da base. */
  const usarRetido = useRef(retido?.adota === true)
  /** d2a: a comparação aberta por esta folha, com o alvo capturado no clique (ou o original, se retida). */
  const [revisao, setRevisao] = useState<SessaoRevisao | null>(() => retido?.adota && origemDaFolha && instalada
    ? { origem: origemDaFolha, id: instalada.id, installationId: retido.r.installationId, nome: instalada.name } : null)

  useLayoutEffect(() => { setConfirmar(null) }, [loja.epoca])

  const selecao = instalada ? { id: instalada.id } : item ? { source: item.id } : null
  /** A lista já diz que o Hub não confere ou não lê o arquivo: nem pergunta o texto. */
  const conteudoDaLista: ConteudoRecusado | null = instalada && instalada.contentStatus !== 'verified' ? instalada.contentStatus : null
  // Uma consulta por versão e tentativa. Outra versão na mesma ID é outra consulta: a anterior é
  // cancelada, e o que ela responder (antes ou depois da nova) não é guardado.
  const consulta = `${versao}#${tentativa}`
  useEffect(() => {
    if (!selecao || conteudoDaLista) return
    let vivo = true
    setLeitura(null)
    loja.detalhes(selecao, origemDaFolha, inspecao).then(
      // `null`: a resposta chegou depois de trocar de Hub, ou a folha abriu sem Hub.
      (d) => { if (vivo) setLeitura(d ? { consulta, detalhes: d, erro: null, codigo: null } : { consulta, detalhes: null, erro: frases.detalhesConexaoMudou, codigo: null }) },
      (e: unknown) => { if (vivo) setLeitura({ consulta, detalhes: null, erro: mensagemDeErro(e), codigo: codigoDeSkill(e) }) },
    )
    return () => { vivo = false }
  }, [consulta])

  // No mesmo render em que a versão muda, a leitura anterior deixa de valer: nada de texto antigo
  // sob registro ou recibo novo, nem no quadro antes do effect.
  const lida = loja.conexaoAtual(origemDaFolha) && (!inspecao || inspecao.valida()) && leitura?.consulta === consulta ? leitura : null
  const daVersao = lida?.detalhes ? detalhesDaVersao(lida.detalhes, item, instalada) : false
  const detalhes = daVersao ? lida!.detalhes : null
  /** O Hub respondeu por outra versão (a lista da página está atrasada): nada dela aparece. */
  const outraNoHub = lida?.detalhes != null && !daVersao
  /** Alterado ou indisponível: pela lista, ou pela resposta dos detalhes de uma linha que a lista ainda dava como conferida. */
  const conteudoRecusado: ConteudoRecusado | null = conteudoDaLista
    ?? (lida?.codigo === 'skill_installation_changed' ? 'changed' : lida?.codigo === 'skill_installation_unavailable' ? 'unavailable' : null)
  const erro = conteudoRecusado ? null : lida?.erro ?? null
  const mudou = versao !== versaoAberta
  // O Dialog mantém a ação durante a saída: ela confere este valor na hora do clique.
  const respostaDeOutra = useRef(outraNoHub)
  useLayoutEffect(() => {
    respostaDeOutra.current = outraNoHub
    if (outraNoHub) setConfirmar(null)
  }, [outraNoHub])

  /** Relê a lista; se o registro continua o mesmo, pergunta o texto outra vez (se mudou, a troca já pergunta). */
  /** `true` só quando a lista foi relida na origem da folha, sem erro, e a versão continua a mesma. */
  const lerDeNovo = async (): Promise<boolean> => {
    if (!loja.conexaoAtual(origemDaFolha) || (inspecao && !inspecao.valida())) return false
    setErroReleitura(null)
    await loja.carregar(inspecao)
    if (!loja.conexaoAtual(origemDaFolha) || (inspecao && !inspecao.valida())) return false
    if (inspecao && (loja.erro || loja.semProtocolo)) { setErroReleitura(loja.erro); return false }
    const agora = instalada
      ? versaoDaFolha(null, loja.instaladas?.find((s) => s.id === instalada.id) ?? null)
      : item ? versaoDaFolha(loja.itens?.find((i) => i.id === item.id) ?? null, null) : null
    if (inspecao && agora !== versao) { setReselecionar(true); return false }
    if (agora !== versao) return false
    setTentativa((n) => n + 1)
    return loja.erro === null && loja.listasDaOrigem(origemDaFolha)
  }

  const instaladas = loja.instaladas ?? []
  const ficha = instalada ? fichaDaInstalada(instalada, detalhes?.catalog ?? null, versaoAtual) : null
  // Pelo catálogo, a versão atual; por uma instalada, só os metadados capturados da versão do recibo.
  const catalogo = ficha ? ficha.capturado : item
  const registro = instalada ?? (item ? instaladaDoItem(item, instaladas) : null)
  const recibo: SkillReceipt | null = ficha ? ficha.recibo : registro?.receipt ?? null
  const estado = item ? estadoDoItem(item, instaladas) : null
  const nome = ficha ? ficha.nome : item?.name ?? ''
  const descricao = ficha ? ficha.descricao : item?.description ?? ''
  const reciboDeOutraVersao = item !== null && recibo !== null && recibo.digest !== item.digest
  const removivel = registro !== null && (instalada !== null || estado?.tipo === 'instalada' || estado?.tipo === 'outraVersao')
  const remocao = registro ? loja.pedido(`rm:${registro.id}`) : null
  const reinstalacao = registro ? loja.pedido(`re:${registro.id}`) : null
  const falhaReinstalar = registro ? loja.falhas.get(`re:${registro.id}`) ?? null : null
  const incertoReinstalarBruto = registro ? loja.incertos.get(`re:${registro.id}`) ?? null : null
  const incertoReinstalar = incertoReinstalarBruto && incertoReinstalarBruto.alvo === registro?.installationId ? incertoReinstalarBruto : null
  const falhaRemocao = registro ? loja.falhas.get(`rm:${registro.id}`) ?? null : null
  const falhaInstalar = item ? loja.falhas.get(item.id) ?? null : null
  /** Pedidos desta folha enviados sem resposta confiável: o de remover vale só para a encarnação que pediu. */
  const incertoInstalar = item ? loja.incertos.get(item.id) ?? null : null
  const incertoRemocaoBruto = registro ? loja.incertos.get(`rm:${registro.id}`) ?? null : null
  const incertoRemocao = incertoRemocaoBruto && incertoRemocaoBruto.alvo === registro?.installationId ? incertoRemocaoBruto : null
  const bloqueada = item !== null && estado?.tipo === 'bloqueada'
  // A conexão em que a folha abriu: depois de uma troca ou reconexão, os botões daqui não agem.
  const conexaoDaFolha = loja.conexaoAtual(origemDaFolha)
  // Com a resposta de outra versão, a folha sabe que a lista está atrasada: não instala nem remove.
  const acoesLivres = conexaoDaFolha && !outraNoHub
  // Só a instalação aberta pela matriz/lista normal. Nada novo na inspeção b2 ou no Catálogo.
  const informativa = !inspecao && item === null && instalada !== null
  // MX-37: uma só conferência e um só uso, presos à origem, conexão e instalação da abertura (guardas existentes).
  const sinaisDaFolha = { mudou, outraNoHub, detalhesDaVersao: detalhes !== null, conteudoRecusado }
  const vigente = leituraVigente(loja, store, origemDaFolha, sinaisDaFolha)
  const estadoConferencia = estadoDaConferencia(vigente, recibo, sinaisDaFolha)
  /** Medição real só da mesma encarnação e numa leitura vigente; ausente ou antiga não vira zero nem "nunca". */
  const uso = vigente && instalada ? loja.instaladas?.find((s) => s.id === instalada.id && s.installationId === instalada.installationId)?.usage ?? null : null
  const resumo = informativa ? resumoDaInstalada(instalada, vigente) : null
  /** A confirmação vale para a instalação que capturou: outra na mesma ID a fecha e a recusa. */
  const confirmacaoVale = (c: { alvo: AlvoRemocao; origem: IntencaoHub }, semOutraNoHub: boolean) =>
    semOutraNoHub && loja.conexaoAtual(c.origem) && !loja.outraInstalacaoNaId(c.alvo)

  // d2a: só manual (sem recibo, origem custom) conferida, fora da inspeção b2 e na conexão da folha.
  const comparavel = !inspecao && rascunho !== undefined && instalada !== null && instalada.receipt === null && instalada.source === 'custom'
    && instalada.contentStatus === 'verified' && conteudoRecusado === null && !mudou && detalhes !== null
  if (revisao) return <ComparaRascunho key={`${revisao.installationId}`} loja={loja} sessao={revisao}
    textoRetido={usarRetido.current && retido ? retido.r.texto : undefined}
    aoMudarTexto={(texto) => rascunho?.guardar({ hub: revisao.origem.hub, id: revisao.id, installationId: revisao.installationId, texto })}
    onVoltar={() => { usarRetido.current = false; rascunho?.guardar(null); setRevisao(null) }} />
  if (inspecao && (!inspecao.valida() || !loja.conexaoAtual(origemDaFolha))) return <Text as="div" role="status" tone="secondary" {...superficieIdioma(idioma)}>{tx(frase('skills.origemMudou'))}</Text>
  if (inspecao && loja.semProtocolo) return <SheetPage><div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}><ModalHeader icon="book.pages" closeTitle={L('fechar')} title={L('titulo')} /><Text as="div" role="status" tone="secondary">{tx(juntas(frase('skills.semProtocoloFrase'), frase('skills.detalhes.fecheAtualize')))}</Text></div></SheetPage>
  if (inspecao && reselecionar) return <SheetPage><div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}><ModalHeader icon="book.pages" closeTitle={L('fechar')} title={L('skillMudou')} /><Text as="div" role="status" tone="secondary">{L('listaAtualizada')}</Text></div></SheetPage>
  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        closeTitle={tx(frase('skills.fechar'))}
        icon="book.pages"
        title={<bdi translate="no">{nome}</bdi>}
        subtitle={catalogo ? <><bdi>{catalogo.category}</bdi> · {tx(rotuloDisponibilidade[catalogo.availability])}</> : recibo ? textos.grupoCatalogo : textos.grupoSemRecibo}
      />
      <CardForm>
        {inspecao && erroReleitura ? <CardSection><VStack gap={8} align="flex-start"><Text tone="danger" role="status">{tx(frase('skills.erroAtualizar', { motivo: erroReleitura }))}</Text><Button kind="secondary" onClick={() => { void lerDeNovo() }}>{L('lerDeNovo')}</Button></VStack></CardSection> : null}
        {retido && !retido.adota ? (
          <CardSection>
            <Text tone="secondary" className="selectable">{retido.destino === 'outroHub' ? textos.revisao.retidoOutroHub : textos.revisao.retidoOutraInstalacao}</Text>
          </CardSection>
        ) : null}
        {conexaoDaFolha ? null : (
          <CardSection>
            <Text tone="danger" className="selectable">{textos.detalhesConexaoMudou}</Text>
          </CardSection>
        )}

        {conteudoRecusado ? (
          <CardSection>
            <VStack gap={8} align="flex-start">
              <Text tone="danger" className="selectable">{informativa ? traduzir(idioma, `skills.acab.estado.${conteudoRecusado}`) : textos.conteudoMotivo[conteudoRecusado]}</Text>
              {inspecao ? null : <Text tone="secondary" className="selectable">{registro?.receipt ? textos.recuperar.curada : textos.recuperar.manual}</Text>}
              <HStack gap={8} wrap>
                {!inspecao && registro?.receipt ? (
                  <Button kind="primary" disabled={reinstalacao !== null || !acoesLivres || incertoReinstalar?.conferido === false} onClick={() => {
                    // A encarnação vista vai no gesto; o Hub recusa se ela já não for a de agora.
                    if (registro && !outraNoHub && loja.conexaoAtual(origemDaFolha)) void loja.reinstalar(registro, origemDaFolha)
                  }}>
                    {reinstalacao ? L('reinstalando') : L('reinstalar')}
                  </Button>
                ) : null}
                <Button kind="secondary" onClick={() => { void lerDeNovo() }}>{L('lerDeNovo')}</Button>
              </HStack>
              {falhaReinstalar ? <Text size="footnote" tone="danger" className="selectable">{tx(falhaReinstalar)}</Text> : null}
              {incertoReinstalar ? <Text size="footnote" tone="attention" className="selectable">{tx(incertoReinstalar.mensagem)}</Text> : null}
            </VStack>
          </CardSection>
        ) : outraNoHub ? (
          <CardSection>
            <VStack gap={8} align="flex-start">
              <Text tone="danger" className="selectable">{L('outraNoHub')}</Text>
              {inspecao ? <Text tone="secondary">{L('fecheAtualizeEscolha')}</Text> : <Button kind="secondary" onClick={() => { void lerDeNovo() }}>{L('lerDeNovo')}</Button>}
            </VStack>
          </CardSection>
        ) : mudou ? (
          <CardSection>
            <Text tone="text" className="selectable">
              {instalada ? L('mudouNoHub') : L('mudouNoCatalogo')}
            </Text>
          </CardSection>
        ) : null}

        {descricao ? (
          <CardSection>
            <Text size="subheadline" tone="secondary" className="selectable" translate="no" dir="auto">{descricao}</Text>
          </CardSection>
        ) : null}

        {!inspecao ? <CardSection footer={comparavel ? textos.revisao.entradaRodape : item && estado?.tipo === 'outraVersao' ? textos.outraVersao : tx(juntas(frases.instalarSoTexto, frase('skills.detalhes.paraUsar', { onde: frases.ondeLigar })))}>
          <HStack gap={8} wrap className="cs-folha-acoes">
            {item && estado ? (
              <>
                <HStack gap={6} wrap><SelosDoItem item={item} estado={estado} /></HStack>
                <span className="cs-espaco" />
                <AcaoInstalar folha item={item} estado={estado} pedido={loja.pedido(item.id)} falhou={falhaInstalar !== null} desligada={!acoesLivres} onInstalar={() => { void loja.instalar(item, origemDaFolha) }} conferir={incertoInstalar && !incertoInstalar.conferido ? () => { void loja.carregar() } : undefined} />
              </>
            ) : (
              <Text size="callout" tone="secondary">{recibo ? L('instaladaDoCatalogo') : textos.semRecibo}</Text>
            )}
            {comparavel ? (
              <Button kind="secondary" disabled={!acoesLivres} onClick={() => {
                // O alvo e a conexão vão no gesto; a página nunca captura a conexão de agora.
                if (!instalada || !acoesLivres || !loja.conexaoAtual(origemDaFolha)) return
                setRevisao({ origem: origemDaFolha, id: instalada.id, installationId: instalada.installationId, nome: instalada.name })
              }}>{textos.revisao.entrada}</Button>
            ) : null}
            {removivel ? (
              <Button kind="secondary" className="cs-remover" disabled={remocao !== null || !acoesLivres || incertoRemocao?.conferido === false} title={incertoRemocao?.conferido === false ? textos.incertoSemRemover : undefined} onClick={() => {
                // A remoção estrita é capturada aqui, no clique, da linha que a folha mostra; nunca depois.
                if (!registro || outraNoHub || !loja.conexaoAtual(origemDaFolha)) return
                const alvo = capturarRemocao(registro)
                if (alvo) setConfirmar({ alvo, origem: origemDaFolha })
              }}>
                {remocao ? tx(frase('skills.removendo')) : L('removerInstalada')}
              </Button>
            ) : null}
          </HStack>
          {falhaInstalar ? <Text size="footnote" tone="danger" className="selectable">{tx(falhaInstalar)}</Text> : null}
          {falhaRemocao ? <Text size="footnote" tone="danger" className="selectable">{tx(falhaRemocao)}</Text> : null}
          {[incertoInstalar, incertoRemocao].map((incerto, n) => incerto ? (
            <Text key={n} size="footnote" tone="attention" className="selectable">{tx(incerto.conferido ? incerto.mensagem : juntas(incerto.mensagem, frases.incertoConferir))}</Text>
          ) : null)}
        </CardSection> : <CardSection><Text tone="secondary">{L('somenteLeitura')}</Text></CardSection>}

        {item && bloqueada && item.reason ? (
          <CardSection title={L('porQueNao')}>
            <Text tone="text" className="selectable">{item.reason}</Text>
          </CardSection>
        ) : null}

        {catalogo && catalogo.availability === 'adapted' ? (
          <CardSection title={L('oQueMuda')}>
            {catalogo.reason ? <Text tone="text" className="selectable">{catalogo.reason}</Text> : <Text tone="secondary">{traduzir(idioma, 'marca.skills.adaptada')}</Text>}
            {catalogo.losses.length ? <Lista titulo={L('ficaDeForaCurto')} itens={catalogo.losses} /> : null}
          </CardSection>
        ) : catalogo && !bloqueada && catalogo.losses.length ? (
          <CardSection title={L('ficaDeFora')}><Lista itens={catalogo.losses} /></CardSection>
        ) : null}

        {catalogo?.requirements.length ? (
          <CardSection title={L('precisaDe')} footer={textos.soOrienta}>
            <Lista itens={catalogo.requirements} />
          </CardSection>
        ) : null}

        {catalogo?.readiness ? (
          <CardSection title={L('execucao')} footer={textos.semPreparar}>
            <ValueRow label={L('estado')}><Text tone="secondary">{L('desligada')}</Text></ValueRow>
            <ValueRow label={L('executor')}><Text tone="secondary">{textos.bloqueada}</Text></ValueRow>
            <VStack gap={6}>
              <Text tone="text" className="selectable">{catalogo.readiness.reason}</Text>
              {catalogo.readiness.unavailableActions.length ? <Lista titulo={L('indisponivelAgora')} itens={catalogo.readiness.unavailableActions} /> : null}
            </VStack>
          </CardSection>
        ) : null}

        {/* Skills: acabamento (b) e (c): a conferência de agora e o último uso, só de uma instalada e fora da inspeção. */}
        {instalada && !inspecao ? <ConferenciaSkill estado={estadoConferencia} recibo={recibo} livre={conexaoDaFolha} onVerificar={lerDeNovo} /> : null}
        {resumo ? <ResumoInformativo resumo={resumo} idioma={idioma} usoMedido={uso !== null} /> : null}
        {instalada && !inspecao ? <UsoDaSkill uso={uso} /> : null}

        {/* Origem da versão que a folha descreve: do item pelo catálogo, do recibo por uma instalada. */}
        {ficha ? (
          recibo ? <Origem origem={recibo.origin} licenca={recibo.license} adaptacao={recibo.adaptation} digest={recibo.digest} arquivos={resumo ? resumo.arquivos ?? [] : recibo.files} informativa={resumo !== null} manifestoIndisponivel={resumo !== null && resumo.arquivos === null} abertos={arquivos} onArquivos={() => setArquivos((v) => !v)} /> : null
        ) : item ? (
          <Origem origem={item.origin} licenca={item.license} adaptacao={item.adaptation} digest={item.digest} arquivos={item.files} abertos={arquivos} onArquivos={() => setArquivos((v) => !v)} />
        ) : null}

        {registro || instalada ? (
          <CardSection title={L(reciboDeOutraVersao ? 'reciboInstalada' : 'recibo')} footer={recibo ? textos.reciboNaoEAuditoria : undefined}>
            {/* Uma linha do cartão por item: CardSection não abre fragmentos, só listas. */}
            {recibo ? [
              reciboDeOutraVersao && registro ? <ValueRow key="nome" label={L('nomeInstalado')}><Text tone="secondary" truncate className="selectable" title={registro.name} translate="no" dir="auto">{registro.name}</Text></ValueRow> : null,
              <ValueRow key="versao" label={L('versaoImportada')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={recibo.digest}>{curto(recibo.digest)}</Text></ValueRow>,
              <ValueRow key="cobertura" label={L('cobertura')}><Text tone="secondary">{textos.cobertura[recibo.coverage] ?? <bdi dir="ltr">{recibo.coverage}</bdi>}</Text></ValueRow>,
              <ValueRow key="licenca" label={L('licenca')}><Text tone="secondary" className="selectable">{recibo.license}</Text></ValueRow>,
              recibo.limitations.length ? <Lista key="limites" titulo={L('limites')} itens={recibo.limitations} /> : null,
            ] : <Text tone="secondary">{textos.semRecibo}</Text>}
          </CardSection>
        ) : null}

        {ficha?.outraVersao ? (
          <CardSection title={L('outraVersaoCatalogo')} footer={textos.outraVersaoNoCatalogo}>
            {[
              <ValueRow key="nome" label={L('nome')}><Text tone="secondary" truncate className="selectable" title={ficha.outraVersao.name} translate="no" dir="auto">{ficha.outraVersao.name}</Text></ValueRow>,
              <ValueRow key="versao" label={L('versaoDigest')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={ficha.outraVersao.digest}>{curto(ficha.outraVersao.digest)}</Text></ValueRow>,
              <ValueRow key="licenca" label={L('licenca')}><Text tone="secondary" className="selectable">{ficha.outraVersao.license}</Text></ValueRow>,
              <ValueRow key="repo" label={L('repositorio')}><Text tone="secondary" truncate className="selectable bidi-isolado" dir="ltr" title={ficha.outraVersao.origin.repo}>{ficha.outraVersao.origin.repo}</Text></ValueRow>,
              <ValueRow key="revisao" label={L('revisao')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={ficha.outraVersao.origin.revision}>{curto(ficha.outraVersao.origin.revision)}</Text></ValueRow>,
            ]}
          </CardSection>
        ) : null}

        <CardSection title={L('instrucoes')} footer={instalada ? textos.instrucoesInstalada : textos.instrucoesCatalogo}>
          {conteudoRecusado ? (
            <Text tone="secondary">{informativa ? traduzir(idioma, 'skills.informativo.semConfirmacaoNota') : textos.conteudoInstrucoes[conteudoRecusado]}</Text>
          ) : detalhes ? (
            detalhes.markdown ? <TextoPassivo texto={detalhes.markdown} /> : <Text tone="secondary">{L('semInstrucoes')}</Text>
          ) : outraNoHub ? (
            <Text tone="secondary">{L('outraVersaoTexto')}</Text>
          ) : erro ? (
            <VStack gap={8} align="flex-start">
              <Text tone="danger" className="selectable">{tx(frase('skills.detalhes.erroTexto', { motivo: erro }))}</Text>
              <Button kind="secondary" onClick={() => { if (loja.conexaoAtual(origemDaFolha) && (!inspecao || inspecao.valida())) setTentativa((n) => n + 1) }}>{tx(frase('skills.tentarDeNovo'))}</Button>
            </VStack>
          ) : <Spinner />}
        </CardSection>
      </CardForm>
      {!inspecao ? <Dialog
        open={confirmar !== null && confirmacaoVale(confirmar, !outraNoHub)}
        title={tx(frase('skills.removerTitulo', { nome: confirmar?.alvo.skill.name ?? nome }))}
        message={confirmar ? <AvisoDeRemocao store={store} skillId={confirmar.alvo.pedido.id} doCatalogo={confirmar.alvo.pedido.expectedReceipt.kind === 'catalog'} /> : null}
        actions={confirmar ? [{ title: tx(frase('skills.remover')), destructive: true, action: () => {
          // A ação pode sobreviver à folha: validar a conexão de origem e a instalação capturada antes de qualquer pedido.
          if (confirmacaoVale(confirmar, !respostaDeOutra.current)) void loja.remover(confirmar.alvo, confirmar.origem)
        } }] : []}
        cancel={tx(frase('skills.cancelar'))}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmar(null)}
      /> : null}
      </div>
    </SheetPage>
  )
}

/** Repositório, revisão, caminho, licença, adaptação, digest e arquivos de uma versão. */
function Origem({ origem, licenca, adaptacao, digest, arquivos, abertos, onArquivos, informativa = false, manifestoIndisponivel = false }: {
  informativa?: boolean
  manifestoIndisponivel?: boolean
  origem: SkillReceipt['origin']
  licenca: string
  adaptacao: string | null
  digest: string
  arquivos: SkillReceipt['files']
  abertos: boolean
  onArquivos: () => void
}) {
  const { idioma, L } = useRotulos()
  return (
    <CardSection title={L('origem')}>
      <ValueRow label={L('repositorio')}><Text tone="secondary" truncate className="selectable bidi-isolado" dir="ltr" title={origem.repo}>{origem.repo}</Text></ValueRow>
      <ValueRow label={L('revisao')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={origem.revision}>{curto(origem.revision)}</Text></ValueRow>
      <ValueRow label={informativa ? traduzir(idioma, 'skills.informativo.caminhoOrigem') : L('caminho')}><Text tone="secondary" truncate className="selectable bidi-isolado" dir="ltr" title={origem.path}>{origem.path}</Text></ValueRow>
      <ValueRow label={L('licenca')}><Text tone="secondary" className="selectable">{licenca}</Text></ValueRow>
      {adaptacao ? <ValueRow label={L('adaptacao')}><Text tone="secondary" className="selectable" dir="auto" title={adaptacao}>{adaptacao}</Text></ValueRow> : null}
      <ValueRow label={L('versaoDigest')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={digest}>{curto(digest)}</Text></ValueRow>
      {informativa ? <Text size="footnote" tone="secondary">{traduzir(idioma, manifestoIndisponivel ? 'skills.informativo.manifestoIndisponivel' : 'skills.informativo.manifestoNota')}</Text> : null}
      {arquivos.length ? (
        <Disclosure expanded={abertos} onToggle={onArquivos} label={<Text tone="text">{informativa ? traduzir(idioma, 'skills.informativo.arquivos') : textoDe(frase('skills.detalhes.arquivos', { n: arquivos.length }), idioma)}</Text>}>
          <VStack gap={4} className="cs-arquivos">
            {arquivos.map((f) => (
              <HStack key={f.path} gap={8}>
                <Text size="caption" mono tone="secondary" truncate className="selectable bidi-isolado" dir="ltr" title={informativa ? undefined : dicaDoArquivo(f)}>{f.path}</Text>
                <span className="cs-espaco" />
                <Text size="caption" tone="secondary" tabular>{tamanho(f.bytes, idioma)}</Text>
              </HStack>
            ))}
          </VStack>
        </Disclosure>
      ) : null}
    </CardSection>
  )
}

// textos-ok: caminho e hash do arquivo (código), iguais em todo idioma.
const dicaDoArquivo = (f: { path: string; sha256: string }) => `${f.path} · sha256 ${f.sha256}`

function Lista({ titulo, itens }: { titulo?: string; itens: string[] }) {
  return (
    <VStack gap={4}>
      {titulo ? <Text size="callout" weight="semibold" tone="secondary">{titulo}</Text> : null}
      <ul className="cs-lista">
        {itens.map((t, i) => <li key={i}><Text tone="text" className="selectable" dir="auto">{t}</Text></li>)}
      </ul>
    </VStack>
  )
}
