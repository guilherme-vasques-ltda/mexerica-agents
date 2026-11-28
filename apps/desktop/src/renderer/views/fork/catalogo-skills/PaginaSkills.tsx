import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react'
import { DropdownMenu, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useStore } from '../../../store/context'
import { FormSheet, MarketSearch } from '../../marketplace/pieces'
import { CatalogoSkills, useCatalogo } from './CatalogoSkills'
import type { InspecaoSkills, IntencaoHub } from './catalogo-store'
import { EscreverSkill } from './EscreverSkill'
import { Anuncio } from './partes'
import { SkillsInstaladas } from './SkillsInstaladas'
import { BaseCentral } from './BaseCentral'
import { alvoDaAba } from './regras'
import { textoDe, frase, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { extensoesSomenteCatalogo } from '../extensoes/tela'

type Aba = 'central' | 'instaladas' | 'catalogo'
const TODAS_ABAS: readonly { id: Aba; rotulo: Frase }[] = [
  { id: 'central', rotulo: frase('skills.aba.central') },
  { id: 'instaladas', rotulo: frase('skills.aba.instaladas') },
  { id: 'catalogo', rotulo: frase('skills.aba.catalogo') },
]
/** A aba escolhida nesta sessão do app; não vira preferência gravada. */
let abaDaSessao: Aba = 'central'
/** Uma abertura da folha "Escrever à mão": a origem do gesto e o número da sessão. */
type SessaoManual = { origem: IntencaoHub; id: number }

/**
 * Extensões → Skills (D166): cabeçalho com busca, atualizar lista e Adicionar; abas Instaladas
 * (lista leve em duas colunas) e Catálogo (linhas de largura inteira). A página é a dona da loja
 * do Hub atual para as duas abas: abre uma vez e troca de Hub com `sincronizar`.
 */
export function PaginaSkills({ busca, onBusca, inspecao }: { busca: string; onBusca: (valor: string) => void; inspecao?: InspecaoSkills }) {
  const store = useStore()
  const loja = useCatalogo(store)
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const hub = store.hello?.hostId ?? null
  const online = store.connection.kind === 'online'
  const [aba, setAba] = useState<Aba>(inspecao ? 'instaladas' : abaDaSessao)
  /** A sessão da folha manual aberta, com a origem (Hub, época e conexão) em que abriu; `null`: fechada. */
  const [escrevendo, setEscrevendo] = useState<SessaoManual | null>(null)
  /**
   * O número da sessão aberta agora, trocado no próprio clique (nunca num effect): fechar e abrir de
   * novo no mesmo tique já invalida a anterior. `0`: nenhuma.
   */
  const sessaoAberta = useRef(0)
  const ultimaSessao = useRef(0)
  const abas = useRef<(HTMLButtonElement | null)[]>([])
  const adicionar = useRef<HTMLSpanElement>(null)
  const uid = useId()
  // A inspeção de uma indicação só lê o catálogo e as instaladas; a Base central (c2) é da página normal.
  const ABAS = inspecao ? TODAS_ABAS.filter((a) => a.id !== 'central') : TODAS_ABAS

  const montado = useRef(false)
  useEffect(() => {
    if (inspecao) {
      void loja.carregar(inspecao)
      return
    }
    if (!montado.current) {
      montado.current = true
      loja.abrir(hub)
    } else loja.sincronizar(hub)
  }, [hub, inspecao])
  // Voltou a conexão sem lista: lê de novo (do mesmo Hub; nunca de outro).
  useEffect(() => {
    if (!inspecao && online && hub !== null && loja.itens === null && !loja.carregando) void loja.carregar()
  }, [online])

  const escolher = (id: Aba, focar = false) => {
    if (!inspecao) abaDaSessao = id
    setAba(id)
    if (focar) abas.current[ABAS.findIndex((a) => a.id === id)]?.focus()
  }
  const teclas = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    const alvo = alvoDaAba(e, i, ABAS.length)
    if (alvo === null) return
    e.preventDefault()
    escolher(ABAS[alvo]!.id, true)
  }
  const abrirEscrever = () => {
    if (inspecao) return
    const origem = loja.capturarIntencao()
    if (!origem) return
    // O menu some depois da ação: a folha devolve o foco ao Adicionar, não à linha do menu.
    adicionar.current?.querySelector('button')?.focus()
    const id = ++ultimaSessao.current
    sessaoAberta.current = id
    setEscrevendo({ origem, id })
  }
  /** Fecha a folha manual; com `id`, só se essa sessão ainda for a aberta. */
  const fecharEscrever = (id?: number) => {
    if (id !== undefined && id !== sessaoAberta.current) return
    sessaoAberta.current = 0
    setEscrevendo(null)
  }

  const dica = `${uid}-dica`
  if (inspecao && (!inspecao.valida() || !loja.conexaoAtual(inspecao.origem))) return <Text as="div" role="status" tone="secondary" {...superficieIdioma(idioma)}>{tx(frase('skills.origemMudou'))}</Text>
  return (
    // D454 (fatia 8b): a página inteira é traduzida e declara o idioma escolhido.
    <section className="cs-pagina idioma-superficie" {...superficieIdioma(idioma)} aria-labelledby={`${uid}-titulo`}>
      <div className="cs-cabeca">
        <VStack gap={4} className="cs-cabeca-textos">
          <Text as="h2" id={`${uid}-titulo`} size="title2" weight="semibold" tone="text">{tx(frase('skills.titulo'))}</Text>
          <Text size="callout" tone="secondary">{inspecao ? tx(frase('skills.inspecao.descricao')) : textos.descricaoPagina}</Text>
        </VStack>
        <div className="cs-cabeca-acoes">
          <MarketSearch className="cs-busca" value={busca} placeholder={tx(frase('skills.buscar'))} onChange={onBusca} />
          <button
            className="icon-button cs-atualizar"
            title={textos.atualizarDica}
            aria-label={tx(frase('skills.atualizarLista'))}
            aria-describedby={dica}
            aria-busy={loja.carregando || undefined}
            onClick={() => void loja.carregar(inspecao)}
          >
            {loja.carregando ? <Spinner size={14} /> : <Icon name="arrow.clockwise" size={14} weight="medium" />}
          </button>
          <span id={dica} className="cs-anuncio">{textos.atualizarDica}</span>
          {!inspecao ? <span ref={adicionar} className="cs-adicionar-ancora">
            <DropdownMenu
              title={tx(frase('skills.adicionarUma'))}
              className="styled-button primary cs-adicionar"
              language={superficieIdioma(idioma)}
              items={() => extensoesSomenteCatalogo()
                ? [{ title: textos.grupoCatalogo, icon: 'arrow.down.to.line', action: () => escolher('catalogo', true) }]
                : [
                  { title: textos.grupoCatalogo, icon: 'arrow.down.to.line', action: () => escolher('catalogo', true) },
                  { title: tx(frase('skills.escreverAMao')), icon: 'square.and.pencil', action: abrirEscrever },
                ]}
            >
              <Icon name="plus" size={12} weight="semibold" />
              <span>{tx(frase('skills.adicionar'))}</span>
              <Icon name="chevron.down" size={10} weight="semibold" />
            </DropdownMenu>
          </span> : null}
        </div>
      </div>

      <Anuncio texto={textoDe(loja.aviso, idioma) ?? ''} />
      <div className="tab-choice cs-abas" role="tablist" aria-label={tx(frase('skills.titulo'))}>
        {ABAS.map((a, i) => (
          <button
            key={a.id}
            ref={(el) => { abas.current[i] = el }}
            id={`${uid}-aba-${a.id}`}
            role="tab"
            aria-selected={a.id === aba}
            aria-controls={`${uid}-painel-${a.id}`}
            tabIndex={a.id === aba ? 0 : -1}
            className={`tab-choice-option${a.id === aba ? ' on' : ''}`}
            onClick={() => escolher(a.id)}
            onKeyDown={(e) => teclas(e, i)}
          >
            {tx(a.rotulo)}
          </button>
        ))}
      </div>

      {/* As abas ficam montadas: categoria escolhida e pedidos não se perdem ao trocar. */}
      {!inspecao ? <div id={`${uid}-painel-central`} role="tabpanel" aria-labelledby={`${uid}-aba-central`} tabIndex={0} hidden={aba !== 'central'} className="cs-painel">
        <BaseCentral busca={busca} onVerCatalogo={() => escolher('catalogo', true)} />
      </div> : null}
      <div id={`${uid}-painel-instaladas`} role="tabpanel" aria-labelledby={`${uid}-aba-instaladas`} tabIndex={0} hidden={aba !== 'instaladas'} className="cs-painel">
        <SkillsInstaladas naPagina busca={busca} inspecao={inspecao} onVerCatalogo={() => escolher('catalogo', true)} />
      </div>
      <div id={`${uid}-painel-catalogo`} role="tabpanel" aria-labelledby={`${uid}-aba-catalogo`} tabIndex={0} hidden={aba !== 'catalogo'} className="cs-painel">
        <CatalogoSkills busca={busca} inspecao={inspecao} />
      </div>

      <Text size="footnote" tone="tertiary">{inspecao ? tx(frase('skills.inspecao.rodape', { onde: frase('skills.ondeLigar') })) : textos.rodapePagina}</Text>
      {!inspecao ? <FormSheet language={superficieIdioma(idioma)} open={escrevendo !== null} onClose={() => fecharEscrever()}>
        {/* Uma folha por sessão: reabrir, mesmo no mesmo tique, monta uma folha nova e vazia. */}
        {escrevendo ? (
          <EscreverSkill
            key={escrevendo.id}
            loja={loja}
            origem={escrevendo.origem}
            aberta={() => sessaoAberta.current === escrevendo.id}
            onConcluir={() => fecharEscrever(escrevendo.id)}
          />
        ) : null}
      </FormSheet> : null}
    </section>
  )
}
