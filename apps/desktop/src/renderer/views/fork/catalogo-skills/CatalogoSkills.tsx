import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { Button, IconButton } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { radioArrowTarget, radioTabStop } from '../../../lib/radio-group'
import { useStore } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import type { CatalogSkill } from '../../../store/fork-skills'
import { EmptyState, FormSheet } from '../../marketplace/pieces'
import { pluginsFor } from '../../marketplace/plugins'
import { catalogoFor, type InspecaoSkills } from './catalogo-store'
import { DetalhesSkill } from './DetalhesSkill'
import { AcaoInstalar, SelosDoItem } from './partes'
import { categorias, estadoDoItem, filtrar, porCategoria, SEM_CATEGORIA, TODAS } from './regras'
import { textoDe, frase, semHub, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

/** A loja do catálogo deste Hub; depois de instalar/remover, os ajustes de bot releem as skills. */
export function useCatalogo(store: BotStore) {
  return useModel(catalogoFor(store, () => store.ready(), () => void pluginsFor(store).refresh()))
}

/**
 * A aba Catálogo da página Skills (D166): a biblioteca curada do Hub atual em uma coluna de
 * largura inteira, por categoria, com busca, detalhes e instalar com um clique. Quem abre e troca
 * a loja de Hub é a página (`PaginaSkills`).
 */
export function CatalogoSkills({ busca, inspecao }: { busca: string; inspecao?: InspecaoSkills }) {
  const store = useStore()
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const loja = useCatalogo(store)
  /** Desconectado de fato (não "conectando"): explica o limite em vez de esperar. */
  const semConexao = store.isOffline
  const semAcesso = semHub(store.connection.kind, store.hostName)
  const motivo = { titulo: tx(semAcesso.titulo), detalhe: tx(semAcesso.detalhe) }
  const erro = loja.erro ? tx(loja.erro) : null
  const tentar = <Button kind="secondary" onClick={() => void loja.carregar(inspecao)}>{tx(frase('skills.tentarDeNovo'))}</Button>
  const [categoria, setCategoria] = useState(TODAS)
  const [aberta, setAberta] = useState<string | null>(null)
  const [exame, setExame] = useState<{ abertura: number; item: CatalogSkill; escopo: InspecaoSkills } | null>(null)
  const sessaoDetalhes = useRef(0)
  useLayoutEffect(() => () => { sessaoDetalhes.current++ }, [])
  const fechar = () => {
    if (inspecao && exame?.abertura !== sessaoDetalhes.current) return
    sessaoDetalhes.current++
    setExame(null)
    setAberta(null)
  }
  const uid = useId()

  const itensLidos = !inspecao || loja.listasDaOrigem(inspecao.origem) ? loja.itens : null
  const itens = itensLidos ?? []
  const instaladas = loja.instaladas ?? []
  // Skills sem categoria: o grupo interno ganha o nome do catálogo no idioma da tela.
  const nomeDaCategoria = (c: string) => (c === SEM_CATEGORIA ? tx(frase('skills.categoriaOutras')) : c)
  const lista = categorias(itens).map((c) => ({ ...c, nome: nomeDaCategoria(c.nome) }))
  const escolhida = categoria === TODAS || lista.some((c) => c.id === categoria) ? categoria : TODAS
  const achados = filtrar(itens, busca, escolhida)
  const query = busca.trim()
  const item = inspecao ? exame?.item ?? null : aberta ? itens.find((i) => i.id === aberta) ?? null : null
  // A origem do que a pessoa vê agora (Hub, época e conexão): um clique que chega depois de trocar
  // de Hub, ou de voltar a ele por outra conexão, não instala nada no Hub de agora.
  const origem = loja.capturarIntencao()

  const linha = (i: CatalogSkill) => {
    const estado = estadoDoItem(i, instaladas)
    return (
      <LinhaCatalogo
        key={i.id}
        item={i}
        estado={estado}
        falha={textoDe(loja.falhas.get(i.id), idioma) ?? null}
        incerto={textoDe(loja.incertos.get(i.id)?.mensagem, idioma) ?? null}
        acao={inspecao ? null : (
          <AcaoInstalar
            item={i}
            estado={estado}
            pedido={loja.pedido(i.id)}
            falhou={loja.falhas.has(i.id)}
            onInstalar={() => { void loja.instalar(i, origem) }}
            conferir={loja.aguardandoConferir(i.id) ? () => { void loja.carregar(inspecao) } : undefined}
          />
        )}
        onAbrir={() => {
          if (!inspecao) { setAberta(i.id); return }
          if (!inspecao.valida() || !loja.conexaoAtual(inspecao.origem)) return
          const id = ++sessaoDetalhes.current
          setExame({ abertura: id, item: structuredClone(i), escopo: { origem: inspecao.origem, valida: () => sessaoDetalhes.current === id && inspecao.valida() } })
        }}
      />
    )
  }

  const corpo = () => {
    if (inspecao && erro) return <EmptyState icon="exclamationmark.triangle" title={tx(frase('skills.erroConsultar'))} detail={erro} action={tentar} />
    if (loja.semProtocolo) return <EmptyState icon="info.circle" title={textos.semProtocolo} detail={textos.semProtocoloDetalhe} />
    if (semConexao && itensLidos === null) {
      return <EmptyState icon="wifi.slash" title={motivo.titulo} detail={motivo.detalhe} action={tentar} />
    }
    if (itensLidos === null) {
      if (erro) return <EmptyState icon="exclamationmark.triangle" title={tx(frase('skills.erroCarregar'))} detail={erro} action={tentar} />
      return <EsqueletoLista />
    }
    if (itens.length === 0) {
      return loja.hubAntigo
        ? <EmptyState icon="info.circle" title={textos.hubAntigo} detail={textos.hubAntigoDetalhe} />
        : <EmptyState toque icon="book.pages" title={tx(frase('skills.catalogoVazio'))} detail={tx(frase('skills.catalogoVazioDetalhe'))} />
    }
    if (achados.length === 0) {
      // Uma frase inteira por caso (busca, categoria), mais a nota do Hub antigo como outra frase.
      const vazio = query
        ? (escolhida === TODAS ? frase('skills.nadaCom', { busca: query }) : frase('skills.nadaComEm', { busca: query, categoria: nomeDaCategoria(escolhida) }))
        : frase('skills.nadaEm', { categoria: nomeDaCategoria(escolhida) })
      return <EmptyState toque icon="magnifyingglass" title={tx(frase('skills.nenhumaEncontrada'))} detail={loja.hubAntigo ? `${tx(vazio)} ${tx(frase('skills.buscaCompativeis'))}` : tx(vazio)} />
    }
    if (escolhida === TODAS && !query) {
      return porCategoria(achados).map((s, n) => (
        <section key={s.categoria} className="cs-categoria" aria-labelledby={`${uid}-cat-${n}`}>
          <Text as="h3" id={`${uid}-cat-${n}`} size="callout" weight="semibold" tone="secondary" className="cs-categoria-titulo" dir="auto">{nomeDaCategoria(s.categoria)}</Text>
          <ListaCatalogo>{s.itens.map(linha)}</ListaCatalogo>
        </section>
      ))
    }
    return <ListaCatalogo>{achados.map(linha)}</ListaCatalogo>
  }

  return (
    <VStack gap={14} className="cs-aba">
      <Text size="callout" tone="secondary">{textos.introCatalogo}</Text>
      {itensLidos !== null && itens.length > 0 ? (
        <Categorias opcoes={[{ id: TODAS, nome: tx(frase('skills.categoriaTodas')), total: itens.length }, ...lista]} selecao={escolhida} onChange={setCategoria} />
      ) : null}
      {!inspecao && loja.instaladaAgora ? (
        <HStack gap={8} className="cs-faixa">
          <Icon name="checkmark" size={12} weight="semibold" />
          <Text size="callout" tone="text" className="cs-faixa-texto">{tx(frase('skills.instaladaAgora', { nome: loja.instaladaAgora, onde: frase('skills.ondeLigar') }))}</Text>
          <IconButton title={tx(frase('skills.fecharAviso'))} icon="xmark" size={22} onClick={() => loja.fecharAviso()} />
        </HStack>
      ) : null}
      {semConexao && itensLidos !== null ? (
        <HStack gap={8} className="cs-faixa erro" role="status">
          <Icon name="wifi.slash" size={12} />
          <Text size="callout" tone="text" className="cs-faixa-texto">{tx(frase(inspecao ? 'skills.offlineDesatualizada' : 'skills.offlineInstalar', { estado: semAcesso.titulo }))}</Text>
        </HStack>
      ) : null}
      {erro && itensLidos !== null ? (
        <HStack gap={8} className="cs-faixa erro" role="alert">
          <Icon name="exclamationmark.triangle" size={12} />
          <Text size="callout" tone="text" className="cs-faixa-texto">{tx(frase('skills.erroAtualizar', { motivo: erro }))}</Text>
          {tentar}
        </HStack>
      ) : null}
      {loja.hubAntigo && itens.length > 0 ? <Text size="footnote" tone="secondary">{tx(frase('skills.formatoAntigo'))}</Text> : null}
      {corpo()}
      <FormSheet language={superficieIdioma(idioma)} open={item !== null} onClose={fechar} width={560} height={680}>
        {item ? <DetalhesSkill key={inspecao ? `exame:${exame?.abertura}` : `${loja.epoca}:${item.id}`} loja={loja} item={item} instalada={null} inspecao={exame?.escopo} /> : null}
      </FormSheet>
    </VStack>
  )
}

/** A lista do catálogo: uma coluna, contorno e divisórias entre as linhas. */
function ListaCatalogo({ children }: { children: ReactNode }) {
  return <ul className="cs-lista-catalogo">{children}</ul>
}

/**
 * Uma linha do catálogo: nome, selos e descrição à esquerda (o botão que abre os detalhes), a ação
 * à direita, fora dele. Nome e descrição vêm do Hub como estão, no idioma da fonte.
 */
function LinhaCatalogo({ item, estado, falha, incerto, acao, onAbrir }: {
  item: CatalogSkill
  estado: ReturnType<typeof estadoDoItem>
  falha: string | null
  /** Pedido enviado sem resposta confiável: o que o Hub pode ou não ter feito, e o que a lista mostra agora. */
  incerto: string | null
  acao: ReactNode
  onAbrir: () => void
}) {
  const idioma = useIdioma()
  const id = useId()
  const descrito = [`${id}-selos`, item.description ? `${id}-desc` : '', falha ? `${id}-falha` : '', incerto ? `${id}-incerto` : ''].filter(Boolean).join(' ')
  return (
    <li className="cs-cat-item">
      <div className="cs-cat-linha">
        <button className="cs-cat-abrir" onClick={onAbrir} aria-label={textoDe(frase('skills.detalhesDe', { nome: item.name }), idioma)} aria-describedby={descrito}>
          <HStack as="span" gap={8} wrap className="cs-cat-titulo">
            <Text size="body" weight="medium" tone="text" truncate title={item.name} translate="no" dir="auto" className="cs-cat-nome">{item.name}</Text>
            <HStack as="span" gap={6} wrap id={`${id}-selos`}><SelosDoItem item={item} estado={estado} /></HStack>
          </HStack>
          {item.description ? <Text id={`${id}-desc`} size="callout" tone="secondary" lines={2} title={item.description} translate="no" dir="auto">{item.description}</Text> : null}
        </button>
        {acao}
      </div>
      {/* Em bloco: aviso longo quebra alinhado ao recuo da linha, não na borda. */}
      {falha ? <Text as="div" id={`${id}-falha`} size="footnote" tone="danger" className="cs-cat-falha selectable">{falha}</Text> : null}
      {incerto ? <Text as="div" id={`${id}-incerto`} size="footnote" tone="attention" className="cs-cat-falha selectable">{incerto}</Text> : null}
    </li>
  )
}

/** Enquanto a lista chega: linhas com a forma das de verdade, paradas com movimento reduzido. */
function EsqueletoLista() {
  const idioma = useIdioma()
  return (
    <ul className="cs-lista-catalogo skeleton" aria-label={textoDe(frase('skills.carregando'), idioma)}>
      {[0, 1, 2, 3].map((i) => (
        <li key={i} className="cs-cat-item cs-cat-linha cs-esqueleto">
          <VStack gap={8} grow>
            <div className="skeleton-title" />
            <div className="skeleton-subtitle cs-esqueleto-desc" />
          </VStack>
          <div className="cs-esqueleto-acao" />
        </li>
      ))}
    </ul>
  )
}

/** Categorias como chips num grupo de rádio (D165): uma parada de Tab, setas movem e escolhem. */
function Categorias({ opcoes, selecao, onChange }: { opcoes: { id: string; nome: string; total: number }[]; selecao: string; onChange: (id: string) => void }) {
  const idioma = useIdioma()
  const t = useT()
  const chips = useRef<(HTMLButtonElement | null)[]>([])
  const parada = radioTabStop(opcoes.map((o) => o.id), selecao)
  const teclas = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    const alvo = radioArrowTarget(e, i, opcoes.length)
    if (alvo === null) return
    e.preventDefault()
    chips.current[alvo]?.focus()
    onChange(opcoes[alvo]!.id)
  }
  return (
    <HStack gap={2} wrap role="radiogroup" aria-label={textoDe(frase('skills.categorias'), idioma)} className="cs-categorias">
      {opcoes.map((o, i) => (
        <button
          key={o.id}
          ref={(el) => { chips.current[i] = el }}
          role="radio"
          aria-checked={o.id === selecao}
          aria-label={textoDe(frase('skills.categoriaTotal', { categoria: o.nome, total: t.plural('skills.contagem', o.total) }), idioma)}
          tabIndex={i === parada ? 0 : -1}
          className={`chip-choice${o.id === selecao ? ' on' : ''}`}
          onClick={() => onChange(o.id)}
          onKeyDown={(e) => teclas(e, i)}
        >
          <bdi>{o.nome}</bdi> <span className="cs-chip-total">{o.total}</span>
        </button>
      ))}
    </HStack>
  )
}
