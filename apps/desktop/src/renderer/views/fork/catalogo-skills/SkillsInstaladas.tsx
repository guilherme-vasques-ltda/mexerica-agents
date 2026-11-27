import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog } from '../../../components/Overlay'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { ThinkingOrb } from '../../../components/ThinkingOrb'
import { useStore } from '../../../store/context'
import type { InstalledSkillV2 } from '../../../store/fork-skills'
import { EmptyState, FormSheet, MarketSection, SkeletonGrid, SkillGlyph } from '../../marketplace/pieces'
import { useCatalogo } from './CatalogoSkills'
import type { InspecaoSkills, IntencaoHub } from './catalogo-store'
import { DetalhesSkill } from './DetalhesSkill'
import { useRetencaoRascunho } from './ComparaRascunho'
import { AvisoDeRemocao, LinhaDeUso } from './Acabamento'
import { Anuncio, Selo } from './partes'
import { LigarEmTodos } from '../extensoes/LigarEmTodos'
import { extensoesSomenteCatalogo } from '../extensoes/tela'
import { agruparInstaladas, capturarRemocao, filtrarInstaladas, type AlvoRemocao, type GrupoInstaladas } from './regras'
import { textoDe, frase, semHub, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

const ROTULO_GRUPO: Record<GrupoInstaladas['id'], 'grupoCatalogo' | 'grupoSemRecibo'> = { catalogo: 'grupoCatalogo', semRecibo: 'grupoSemRecibo' }

/**
 * O que o Hub atual tem instalado, em lista leve de duas colunas (uma no estreito), agrupado só
 * pelo que os dados dizem: com recibo do catálogo ou sem recibo (antigas e escritas à mão).
 * Remover pede confirmação e manda a encarnação e o recibo esperado capturados no clique.
 *
 * Na página Skills (`naPagina`), a página abre a loja, anuncia e mostra carregando/vazio/sem
 * conexão; em Extensões → instalados, este componente faz isso sozinho, como antes.
 */
export function SkillsInstaladas({ naPagina = false, busca = '', onVerCatalogo, inspecao }: {
  inspecao?: InspecaoSkills
  naPagina?: boolean
  busca?: string
  /** Leva à aba Catálogo (estado vazio da página). */
  onVerCatalogo?: () => void
}) {
  const store = useStore()
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const t = useT()
  const tx = (f: Frase) => textoDe(f, idioma)
  const loja = useCatalogo(store)
  const hub = store.hello?.hostId ?? null
  const [aberta, setAberta] = useState<string | null>(null)
  const [confirmar, setConfirmar] = useState<{ alvo: AlvoRemocao; origem: IntencaoHub } | null>(null)
  const titulo = useRef<HTMLHeadingElement>(null)
  const linhas = useRef(new Map<string, HTMLButtonElement>())
  /** A linha removida a partir daqui: quando ela some, o foco vai para a vizinha (ou o título). */
  const removida = useRef<{ id: string; indice: number } | null>(null)
  const [exame, setExame] = useState<{ abertura: number; skill: InstalledSkillV2; escopo: InspecaoSkills } | null>(null)
  const sessaoDetalhes = useRef(0)
  /** D2A-UI1: o texto da comparação desta abertura, só em memória; sai ao fechar ou abrir outra. */
  const rascunhoRetido = useRetencaoRascunho()
  const retencao = rascunhoRetido.retencao
  useLayoutEffect(() => () => { sessaoDetalhes.current++ }, [])
  const fechar = () => {
    if (inspecao && exame?.abertura !== sessaoDetalhes.current) return
    sessaoDetalhes.current++
    rascunhoRetido.limpar()
    setExame(null)
    setAberta(null)
  }
  const uid = useId()

  const montado = useRef(false)
  useEffect(() => {
    if (naPagina || inspecao) return
    if (!montado.current) {
      montado.current = true
      loja.abrir(hub)
    } else loja.sincronizar(hub)
  }, [hub])

  useLayoutEffect(() => {
    setConfirmar(null)
    removida.current = null
  }, [hub, loja.epoca])

  const instaladasLidas = !inspecao || loja.listasDaOrigem(inspecao.origem) ? loja.instaladas : null
  const lista = instaladasLidas ?? []
  const grupos = agruparInstaladas(filtrarInstaladas(lista, busca))
  /** A ordem em que as linhas aparecem (grupos e busca): é nela que o foco procura a vizinha. */
  const visiveis = grupos.flatMap((g) => g.skills)
  const origemDaLista = loja.capturarIntencao()
  const abertaIndice = useRef(0)
  useLayoutEffect(() => {
    // Removida pela folha de detalhes: fecha a folha e trata como removida daqui.
    if (aberta && loja.instaladas && !lista.some((s) => s.id === aberta)) {
      removida.current = { id: aberta, indice: abertaIndice.current }
      rascunhoRetido.limpar()
      setAberta(null)
    }
    const alvo = removida.current
    if (!alvo) return
    if (lista.some((s) => s.id === alvo.id)) {
      // Falhou ou ainda está indo: o foco continua na lixeira dela.
      if (!loja.pedido(`rm:${alvo.id}`)) removida.current = null
      return
    }
    removida.current = null
    const vizinha = visiveis[Math.min(alvo.indice, visiveis.length - 1)]
    // Depois que a folha ou o diálogo devolverem a página (deixa de ser `inert`).
    setTimeout(() => {
      const ativo = document.activeElement
      if (ativo && ativo !== document.body) return
      ;((vizinha ? linhas.current.get(vizinha.id) : null) ?? titulo.current)?.focus()
    }, 0)
  })

  /** O aviso incerto de uma remoção vale só para a encarnação que ela pediu. */
  const incertoDaLinha = (s: InstalledSkillV2) => {
    const incerto = loja.incertos.get(`rm:${s.id}`)
    return incerto && incerto.alvo === s.installationId ? { mensagem: textoDe(incerto.mensagem, idioma), aguardando: !incerto.conferido } : null
  }
  const aberto = inspecao ? exame?.skill ?? null : aberta ? lista.find((s) => s.id === aberta) ?? null : null
  // Só para o cartão "Outra versão no catálogo": os detalhes falam do registro e do recibo abertos.
  const doCatalogo = aberto?.receipt ? loja.itens?.find((i) => i.id === aberto.receipt!.catalogId) ?? null : null
  if (!naPagina && instaladasLidas === null && !loja.erro) return null

  const tentar = <Button kind="secondary" onClick={() => void loja.carregar(inspecao)}>{tx(frase('skills.tentarDeNovo'))}</Button>
  const semAcesso = semHub(store.connection.kind, store.hostName)
  const motivo = { titulo: tx(semAcesso.titulo), detalhe: tx(semAcesso.detalhe) }
  const erro = loja.erro ? tx(loja.erro) : null
  const corpo = (): ReactNode => {
    if (inspecao && erro) return <EmptyState icon="exclamationmark.triangle" title={tx(frase('skills.erroConsultar'))} detail={erro} action={tentar} />
    if (loja.semProtocolo) return <EmptyState icon="info.circle" title={textos.semProtocolo} detail={textos.semProtocoloDetalhe} />
    if (instaladasLidas === null) {
      if (store.isOffline) return <EmptyState icon="wifi.slash" title={motivo.titulo} detail={motivo.detalhe} action={tentar} />
      if (erro) return <EmptyState icon="exclamationmark.triangle" title={tx(frase('skills.erroLer'))} detail={erro} action={tentar} />
      return <SkeletonGrid />
    }
    if (lista.length === 0) {
      return naPagina
        ? <EmptyState icon="book.pages" title={textos.nenhumaInstalada} detail={textos.nenhumaInstaladaDetalhe} action={onVerCatalogo ? <Button kind="secondary" onClick={onVerCatalogo}>{tx(frase('skills.verCatalogo'))}</Button> : undefined} />
        : <Text tone="secondary">{tx(frase('skills.nenhumaInstaladaFrase'))}</Text>
    }
    if (visiveis.length === 0) {
      return <EmptyState toque icon="magnifyingglass" title={tx(frase('skills.nenhumaInstaladaEncontrada'))} detail={tx(frase('skills.nadaCom', { busca: busca.trim() }))} />
    }
    return grupos.map((g) => {
      const cabeca = `${uid}-${g.id}`
      return (
        <section key={g.id} className="cs-grupo" aria-labelledby={cabeca}>
          <VStack gap={2} className="cs-grupo-cabeca">
            <Text as="h3" id={cabeca} size="callout" weight="semibold" tone="text" className="cs-grupo-titulo">
              <span>{textos[ROTULO_GRUPO[g.id]]}</span>
              <span className="cs-anuncio">{', '}{t.plural('skills.contagem', g.skills.length)}</span>
              <Text size="callout" weight="regular" tone="secondary" tabular aria-hidden>{g.skills.length}</Text>
            </Text>
            {g.id === 'semRecibo' ? <Text size="footnote" tone="secondary">{textos.notaSemRecibo}</Text> : null}
          </VStack>
          <div className="cs-grade-instaladas">
            {g.skills.map((s) => (
              <LinhaInstalada
                key={s.id}
                skill={s}
                grupo={cabeca}
                pedido={loja.pedido(`rm:${s.id}`) !== null}
                falha={textoDe(loja.falhas.get(`rm:${s.id}`), idioma) ?? null}
                incerto={incertoDaLinha(s)}
                linha={(el) => { if (el) linhas.current.set(s.id, el); else linhas.current.delete(s.id) }}
                somenteLeitura={!!inspecao}
                aoMudarNovosBots={() => void loja.carregar()}
                onAbrir={() => {
                  if (!inspecao) { abertaIndice.current = visiveis.indexOf(s); rascunhoRetido.limpar(); setAberta(s.id); return }
                  if (!inspecao.valida() || !loja.conexaoAtual(inspecao.origem)) return
                  const id = ++sessaoDetalhes.current
                  setExame({ abertura: id, skill: structuredClone(s), escopo: { origem: inspecao.origem, valida: () => sessaoDetalhes.current === id && inspecao.valida() } })
                }}
                onRemover={() => {
                  // A remoção estrita é capturada aqui, no clique, da linha vista; nunca depois.
                  if (!loja.conexaoAtual(origemDaLista)) return
                  const alvo = capturarRemocao(s)
                  if (alvo) setConfirmar({ alvo, origem: origemDaLista })
                }}
              />
            ))}
          </div>
        </section>
      )
    })
  }

  const conteudo = (
    <>
      <h3 ref={titulo} tabIndex={-1} className="cs-titulo-oculto">{tx(frase('skills.instaladasTitulo'))}</h3>
      {naPagina ? null : <Anuncio texto={textoDe(loja.aviso, idioma) ?? ''} />}
      {erro && instaladasLidas !== null ? (
        <HStack gap={8} className="cs-faixa erro" role="alert">
          <Icon name="exclamationmark.triangle" size={12} />
          <Text size="callout" tone="text" className="cs-faixa-texto">{tx(frase('skills.erroLerMotivo', { motivo: erro }))}</Text>
          {tentar}
        </HStack>
      ) : null}
      {naPagina && store.isOffline && instaladasLidas !== null ? (
        <HStack gap={8} className="cs-faixa erro" role="status">
          <Icon name="wifi.slash" size={12} />
          <Text size="callout" tone="text" className="cs-faixa-texto">{tx(frase(inspecao ? 'skills.offlineDesatualizada' : 'skills.offlineRemover', { estado: semAcesso.titulo }))}</Text>
        </HStack>
      ) : null}
      {corpo()}
      {!inspecao ? <Dialog
        // Presa à conexão e à instalação que a pessoa viu: outra instalação na mesma ID fecha a confirmação.
        open={confirmar !== null && loja.conexaoAtual(confirmar.origem) && !loja.outraInstalacaoNaId(confirmar.alvo)}
        title={tx(frase('skills.removerTitulo', { nome: confirmar?.alvo.skill.name ?? '' }))}
        // Skills: acabamento (a): quem perde a skill aparece aqui, antes do gesto.
        message={confirmar ? <AvisoDeRemocao store={store} skillId={confirmar.alvo.pedido.id} doCatalogo={confirmar.alvo.pedido.expectedReceipt.kind === 'catalog'} /> : null}
        actions={confirmar ? [{ title: tx(frase('skills.remover')), destructive: true, action: () => {
          // O Dialog pode guardar a ação enquanto fecha; nem um callback tardio ganha outra época ou outra instalação.
          if (!loja.conexaoAtual(confirmar.origem) || loja.outraInstalacaoNaId(confirmar.alvo)) return
          const id = confirmar.alvo.pedido.id
          removida.current = { id, indice: visiveis.findIndex((s) => s.id === id) }
          void loja.remover(confirmar.alvo, confirmar.origem)
        } }] : []}
        cancel={tx(frase('skills.cancelar'))}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmar(null)}
      /> : null}
      <FormSheet language={superficieIdioma(idioma)} open={aberto !== null} onClose={fechar} width={560} height={680}>
        {aberto ? <DetalhesSkill key={inspecao ? `exame:${exame?.abertura}` : `${loja.epoca}:${aberto.id}`} loja={loja} item={null} instalada={aberto} versaoAtual={doCatalogo} inspecao={exame?.escopo} rascunho={inspecao ? undefined : retencao} /> : null}
      </FormSheet>
    </>
  )

  // Fora da página Skills (Instalados do Marketplace), a seção declara o idioma: o resto da folha pode ser legado.
  return naPagina ? <VStack gap={16} className="cs-aba">{conteudo}</VStack> : <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}><MarketSection title={tx(frase('skills.titulo'))}>{conteudo}</MarketSection></div>
}

/**
 * Uma instalada: tile, nome e descrição (o botão de detalhes) e a lixeira ao lado, fora dele. Conteúdo
 * alterado ou indisponível no Hub ganha selo com texto e deixa a lixeira desligada, com o motivo.
 */
function LinhaInstalada({ somenteLeitura, aoMudarNovosBots, skill, grupo, pedido, falha, incerto, linha, onAbrir, onRemover }: {
  somenteLeitura?: boolean
  /** MX-264: depois de mexer em "Novos bots já vêm com esta skill", a lista é lida de novo. */
  aoMudarNovosBots?: () => void
  skill: InstalledSkillV2
  /** O título do grupo, que diz se ela tem recibo. */
  grupo: string
  pedido: boolean
  falha: string | null
  /** Remoção enviada sem resposta confiável; `aguardando`: a lista ainda não foi relida depois. */
  incerto: { mensagem: string; aguardando: boolean } | null
  linha: (el: HTMLButtonElement | null) => void
  onAbrir: () => void
  onRemover: () => void
}) {
  const id = useId()
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const conteudo = skill.contentStatus === 'verified' ? null : skill.contentStatus
  // D306: alterada ou ilegível também se remove; o Hub confere encarnação e recibo.
  const inerte = pedido || incerto?.aguardando === true
  const nome = <Text size="body" weight="medium" tone="text" truncate title={skill.name} translate="no" dir="auto" className={conteudo ? 'cs-cat-nome' : undefined}>{skill.name}</Text>
  return (
    <VStack gap={2} className="cs-item">
      <div className="cs-linha">
        <button
          ref={linha}
          className="cs-linha-abrir"
          aria-label={textoDe(frase('skills.detalhesDe', { nome: skill.name }), idioma)}
          aria-describedby={[grupo, conteudo ? `${id}-conteudo` : '', skill.description ? `${id}-desc` : '', falha ? `${id}-falha` : '', incerto ? `${id}-incerto` : ''].filter(Boolean).join(' ')}
          onClick={onAbrir}
        >
          <span className="market-row-icon cs-tile"><SkillGlyph /></span>
          <VStack as="span" gap={2} grow className="cs-linha-textos">
            {conteudo ? (
              <HStack as="span" gap={8} wrap className="cs-cat-titulo">
                {nome}
                <Selo tom="aviso" id={`${id}-conteudo`}>{textos.conteudo[conteudo]}</Selo>
              </HStack>
            ) : nome}
            {skill.description ? <Text id={`${id}-desc`} size="callout" tone="secondary" lines={2} title={skill.description} translate="no" dir="auto">{skill.description}</Text> : null}
            <LinhaDeUso uso={skill.usage} />
          </VStack>
        </button>
        {/* O mesmo botão enquanto remove: o foco que voltou do diálogo fica nele. */}
        {!somenteLeitura ? <button
          title={pedido ? textoDe(frase('skills.removendo'), idioma) : incerto?.aguardando ? textos.incertoSemRemover : textoDe(frase('skills.remover'), idioma)}
          aria-label={textoDe(frase(pedido ? 'skills.removendoNome' : 'skills.removerNome', { nome: skill.name }), idioma)}
          aria-describedby={conteudo ? `${id}-conteudo` : undefined}
          aria-disabled={inerte ? true : undefined}
          aria-busy={pedido ? true : undefined}
          className="market-remove cs-remover-linha"
          onClick={() => { if (!inerte) onRemover() }}
        >
          {pedido ? <ThinkingOrb size={14} color="currentColor" /> : <Icon name="trash" size={13} />}
        </button> : null}
      </div>
      {/* MX-264: "Ligar em todos os bots" e "Novos bots já vêm com esta skill"; o Hub só liga skill com o conteúdo conferido. */}
      {!somenteLeitura && !extensoesSomenteCatalogo() && skill.contentStatus === 'verified' ? <LigarEmTodos tipo="skill" id={skill.id} nome={skill.name} novosBots={skill.novosBots === true} aoMudarNovosBots={aoMudarNovosBots} /> : null}
      {/* Em bloco: aviso longo quebra alinhado ao texto da linha, não na borda. */}
      {falha ? <Text as="div" id={`${id}-falha`} size="footnote" tone="danger" className="cs-linha-falha selectable">{falha}</Text> : null}
      {incerto ? <Text as="div" id={`${id}-incerto`} size="footnote" tone="attention" className="cs-linha-falha selectable">{incerto.aguardando ? `${incerto.mensagem} ${textos.incertoConferir}` : incerto.mensagem}</Text> : null}
    </VStack>
  )
}
