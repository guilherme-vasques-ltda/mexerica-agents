// Skills c2 (D288): a aba Base central da página Skills. Uma tabela Skill × Bot do Hub desta tela, só
// do Pessoal. Cada célula mostra o vínculo de `cfg.skills`; mudar é o gesto c1 (`captureSkillChoice`),
// uma célula por vez, com o valor só mudando quando o Hub confirma. Instalar continua no Catálogo.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { isGroup, type Bot } from '@shared/models'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { anchorPlacement, useEscape } from '../../../components/Overlay'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { useStore } from '../../../store/context'
import { captureSkillChoice } from '../../../store/fork-bot-settings-intents'
import type { InstalledSkillV2 } from '../../../store/fork-skills'
import { EmptyState, FormSheet, MarketSearch, SkeletonGrid } from '../../marketplace/pieces'
import { linhasDaMatriz, MarcasMatriz, textoInverso, TEXTOS_MATRIZ, type Celula, type Leitura, type Linha, type OrigemMatriz } from './base-central'
import { useCatalogo } from './CatalogoSkills'
import { DetalhesSkill } from './DetalhesSkill'
import { useRetencaoRascunho } from './ComparaRascunho'
import { filtrarInstaladas } from './regras'
import { textoDe, cru, frase, juntas, plural, semHub, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'

const marcasPorStore = new WeakMap<BotStore, MarcasMatriz>()
function marcasDe(store: BotStore) {
  let m = marcasPorStore.get(store)
  if (!m) marcasPorStore.set(store, (m = new MarcasMatriz()))
  return m
}

/** A conexão viva desta tela (Hub, link e geração), a mesma tupla que o c1 confere. */
function origemDe(store: BotStore): OrigemMatriz | null {
  const hub = store.hello?.hostId
  return hub && store.connectedClient ? { hub, cliente: store.connectedClient, geracao: store.principalEpoch } : null
}

/** Colunas: os bots do Pessoal deste Hub, sem grupos, na ordem da barra lateral e depois os ocultos. */
export function botsDoPessoal(store: Pick<BotStore, 'roster' | 'hiddenBots'>): Bot[] {
  return [...store.roster, ...store.hiddenBots].filter((b) => !isGroup(b) && (b.spaceId === undefined || b.spaceId === 'personal'))
}

const simNao = (ligada: boolean) => frase(ligada ? 'skills.matriz.sim' : 'skills.matriz.nao')

export function BaseCentral({ busca, onVerCatalogo }: { busca: string; onVerCatalogo: () => void }) {
  const store = useStore()
  const loja = useCatalogo(store)
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const marcas = useModel(marcasDe(store))
  const [aberta, setAberta] = useState<string | null>(null)
  /** D2A-UI1: o texto da comparação desta abertura, só em memória; sai ao fechar ou abrir outra. */
  const rascunhoRetido = useRetencaoRascunho()
  const retencao = rascunhoRetido.retencao
  const [filtroBots, setFiltroBots] = useState('')
  const [dica, setDica] = useState<Dica | null>(null)
  const origem = origemDe(store)
  const online = store.connection.kind === 'online'
  // Outra conexão descarta as marcas da anterior (sem reenviar nada).
  useLayoutEffect(() => { marcas.sincronizar(origem) }, [origem?.hub, origem?.cliente, origem?.geracao])
  const intencao = loja.capturarIntencao()
  const fresca = online && origem !== null && intencao !== null && loja.listasDaOrigem(intencao)
  // Reconectou com a lista de antes: relê nesta conexão antes de oferecer qualquer gesto.
  useEffect(() => {
    if (online && origem && loja.instaladas !== null && !fresca && !loja.carregando) void loja.carregar()
  }, [online, origem?.cliente, origem?.geracao])

  const bots = botsDoPessoal(store)
  const leitura: Leitura | null = fresca ? 'vivo' : loja.instaladas !== null && store.hello !== null && !online ? 'guardado' : null
  // Sem as duas leituras do mesmo Hub (instaladas e bots), nenhuma célula: carregando ou erro não viram Não.
  const linhas = leitura && loja.instaladas ? linhasDaMatriz(loja.instaladas, bots, marcas, leitura) : null
  const achadas = new Set(filtrarInstaladas(loja.instaladas ?? [], busca).map((s) => s.id))
  const termo = busca.trim().toLowerCase()
  const visiveis = (linhas ?? []).filter((l) => (l.skill ? achadas.has(l.id) : !termo || l.id.toLowerCase().includes(termo)))

  if (store.spaceId !== undefined && store.spaceId !== 'personal') {
    return <EmptyState icon="info.circle" title={tx(frase('skills.matriz.soPessoal'))} detail={tx(frase('skills.matriz.soPessoalDetalhe'))} />
  }
  if (loja.semProtocolo) return <EmptyState icon="info.circle" title={textos.semProtocolo} detail={textos.semProtocoloDetalhe} />
  const tentar = <Button kind="secondary" onClick={() => void loja.carregar()}>{tx(frase('skills.tentarDeNovo'))}</Button>
  if (!linhas) {
    if (store.isOffline) {
      const motivo = semHub(store.connection.kind, store.hostName)
      return <EmptyState icon="wifi.slash" title={tx(motivo.titulo)} detail={tx(frase('skills.matriz.reconecteParaVer'))} action={tentar} />
    }
    if (loja.erro) return <EmptyState icon="exclamationmark.triangle" title={tx(frase('skills.erroLer'))} detail={tx(loja.erro)} action={tentar} />
    return <VStack gap={8}><Text role="status" tone="secondary">{tx(frase('skills.matriz.carregando'))}</Text><SkeletonGrid /></VStack>
  }
  if (linhas.length === 0) {
    return <EmptyState icon="book.pages" title={textos.nenhumaInstalada} detail={tx(frase('skills.matriz.vaziaDetalhe'))}
      action={<Button kind="secondary" onClick={onVerCatalogo}>{tx(frase('skills.verCatalogo'))}</Button>} />
  }
  if (bots.length === 0) return <EmptyState icon="person.2" title={tx(frase('skills.matriz.semBots'))} detail={tx(frase('skills.matriz.semBotsDetalhe'))} />

  const alternar = (bot: Bot, skill: InstalledSkillV2, ligar: boolean) => {
    const agora = origemDe(store)
    if (!agora || leitura !== 'vivo' || marcas.salvandoNoBot(bot)) return
    marcas.sincronizar(agora)
    // c1: versão do bot, instalação vista e origem capturadas aqui, no clique, antes de qualquer await.
    const escolha = captureSkillChoice(store, bot, skill, ligar)
    // D507: a recusa fica como dado (a `Mensagem` do Hub já é uma `Frase`) e se traduz ao desenhar (Kodus do #572).
    if ('refused' in escolha) { marcas.recusarNoGesto(bot, skill.id, escolha.refused); return }
    if (!marcas.iniciar(agora, bot, skill.id, ligar)) return
    escolha.send().then(
      () => marcas.terminar(agora, bot.id, skill.id, { ok: true }),
      (erro: unknown) => marcas.terminar(agora, bot.id, skill.id, { erro, mensagem: cru(erro instanceof Error ? erro.message : String(erro)) }),
    )
  }
  const conferir = (bot: string, skill: string) => {
    const agora = origemDe(store)
    if (!agora) return
    // Só uma releitura bem-sucedida nesta mesma conexão confere; falha deixa "sem confirmação".
    void loja.carregar().then(() => {
      const lida = loja.capturarIntencao()
      if (!loja.erro && lida !== null && loja.listasDaOrigem(lida)) marcas.conferir(agora, bot, skill)
    })
  }

  const status = leitura ? textoDe(resumo(linhas, bots, marcas.aviso, leitura), idioma) ?? null : null
  const detalhada = aberta ? loja.instaladas?.find((s) => s.id === aberta) ?? null : null
  const doCatalogo = detalhada?.receipt ? loja.itens?.find((i) => i.id === detalhada.receipt!.catalogId) ?? null : null
  // Filtro só visual das colunas: a projeção, o inverso e o gesto continuam sobre todos os bots.
  const termoBots = filtroBots.trim().toLowerCase()
  const colunas = bots.map((b, i) => ({ bot: b, i })).filter(({ bot }) => !termoBots || bot.name.toLowerCase().includes(termoBots))
  const abrirDica = (alvo: HTMLElement, conteudo: Omit<Dica, 'alvo'>) => setDica({ ...conteudo, alvo })
  const lerDeNovo = (bot: string, skill: string) => {
    const agora = origemDe(store)
    if (!agora) return
    void loja.carregar().then(() => {
      const lida = loja.capturarIntencao()
      if (!loja.erro && lida !== null && loja.listasDaOrigem(lida)) marcas.dispensarRecusa(agora, bot, skill)
    })
  }

  return (
    <VStack gap={12} className="bc-aba">
      <HStack gap={12} wrap className="bc-escopo">
        <span className="bc-pilula">{tx(frase('skills.matriz.pessoalHub', { hub: store.hostName }))}</span>
        <Text size="footnote" tone="secondary">{tx(frase('skills.matriz.escopo', {
          skills: plural('skills.contagem', linhas.length),
          bots: termoBots ? plural('skills.matriz.botsDe', bots.length, { mostrados: colunas.length }) : plural('skills.matriz.bots', bots.length),
        }))}</Text>
        <MarketSearch className="bc-filtro" value={filtroBots} placeholder={tx(frase('skills.matriz.filtrarBots'))} onChange={setFiltroBots} />
        <Text size="footnote" className="bc-rolar" aria-hidden>{tx(frase('skills.matriz.role'))}</Text>
      </HStack>
      <div className={`bc-faixa${status ? '' : ' vazia'}`} role="status" aria-live="polite">{status}</div>
      {visiveis.length === 0 ? <EmptyState toque icon="magnifyingglass" title={tx(frase('skills.nenhumaEncontrada'))} detail={tx(frase('skills.nadaCom', { busca: busca.trim() }))} />
        : colunas.length === 0 ? <EmptyState icon="magnifyingglass" title={tx(frase('skills.matriz.nenhumBot'))} detail={tx(frase('skills.nadaCom', { busca: filtroBots.trim() }))} /> : (
        <div className="bc-matriz" tabIndex={0} role="region" aria-label={tx(frase('skills.matriz.regiao'))}>
          {/* Largura fixa pela soma das colunas: texto longo não alarga a tabela. */}
          <table style={{ '--bc-n': colunas.length } as CSSProperties}>
            <colgroup><col className="bc-col-skill" />{colunas.map(({ bot }) => <col key={bot.id} className="bc-col-bot" />)}</colgroup>
            <thead>
              <tr>
                <th scope="col" className="bc-canto">{tx(frase('skills.matriz.skill'))}</th>
                {colunas.map(({ bot }) => {
                  const salvando = marcas.salvandoNoBot(bot)
                  // Nome e título do bot (dados) e o estado, separados por " · " como numa lista.
                  const nome = [bot.name, bot.title, salvando ? tx(frase('skills.matriz.salvandoUma')) : null].filter(Boolean).join(' · ')
                  return (
                    <th key={bot.id} scope="col" abbr={bot.name}>
                      {/* Foco de teclado mostra a mesma dica do ponteiro; o leitor lê o nome inteiro. */}
                      <span className="bc-bot" tabIndex={0} aria-label={nome}>
                        <span className="bc-rosto">
                          <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(bot), bot.name, bot.description)} size={22} label="" animated={false} />
                          {salvando ? <span className="bc-ponto" aria-hidden /> : null}
                        </span>
                        <span className="bc-bot-nome" translate="no" dir="auto" aria-hidden>{bot.name}</span>
                        <span className="bc-bot-dica" role="tooltip" aria-hidden>{nome}</span>
                      </span>
                    </th>
                  )
                })}
              </tr>
            </thead>
            <tbody>
              {visiveis.map((l) => (
                <tr key={l.id}>
                  <th scope="row">
                    {/* Linhas baixas (D304): nome e descrição numa linha cada; aviso e contagem como ícones com o texto inteiro. */}
                    <span className="bc-linha1">
                      {l.skill
                        ? <button className="bc-nome" translate="no" dir="auto" title={l.nome} aria-label={tx(frase('skills.detalhesDe', { nome: l.nome }))} onClick={() => { rascunhoRetido.limpar(); setAberta(l.id) }}>{l.nome}</button>
                        : <span className="bc-nome" translate="no" dir="auto" title={l.nome}>{l.nome}</span>}
                      {l.aviso ? <IconeEstado icone={l.aviso === 'orfa' ? 'minus.circle' : 'info.circle'} tom="neutro" titulo={AVISOS[l.aviso]} texto={frase('skills.matriz.nomeTexto', { nome: l.nome, texto: AVISO_DETALHE[l.aviso] })} onDica={abrirDica} /> : null}
                      <Contagem linha={l} onDica={abrirDica} />
                    </span>
                    {l.descricao ? <span className="bc-sub bc-desc" dir="auto" title={l.descricao}>{l.descricao}</span> : null}
                  </th>
                  {colunas.map(({ bot, i }) => {
                    const c = l.celulas[i]!
                    return (
                      <td key={bot.id} className={c.tipo === 'incerta' || (c.tipo === 'controle' && c.recusa) ? 'atencao' : undefined}>
                        <CelulaMatriz celula={c} skill={l.nome} bot={bot.name}
                          onAlternar={(ligar) => { if (l.skill) alternar(bot, l.skill as InstalledSkillV2, ligar) }}
                          onConferir={() => conferir(bot.id, l.id)} onLerDeNovo={() => lerDeNovo(bot.id, l.id)} onDica={abrirDica} />
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <HStack gap={16} wrap justify="space-between" className="bc-legenda">
        <Text size="footnote" tone="secondary"><b>{tx(frase('skills.matriz.legendaSim'))}</b> {tx(frase('skills.matriz.legendaResto'))}</Text>
        <Text size="footnote" tone="secondary">{tx(frase('skills.matriz.legendaIcone'))}</Text>
      </HStack>
      <Popover dica={dica} onClose={() => setDica(null)} />
      <FormSheet language={superficieIdioma(idioma)} open={detalhada !== null} onClose={() => { rascunhoRetido.limpar(); setAberta(null) }} width={560} height={680}>
        {detalhada ? <DetalhesSkill key={`${loja.epoca}:${detalhada.id}`} loja={loja} item={null} instalada={detalhada} versaoAtual={doCatalogo} rascunho={retencao} /> : null}
      </FormSheet>
    </VStack>
  )
}

/** O que um ícone de estado abre: o texto inteiro e, quando houver, a ação de reler no mesmo destino. */
// D507 (R1-02 do #579): a dica guarda frases (dado) e o popover traduz ao desenhar; trocar o idioma com ela aberta troca o texto.
export type Dica = { alvo: HTMLElement; titulo: Frase; texto: Frase; acao?: { rotulo: Frase; fazer: () => void } }

/** Popover ancorado no ícone (mesma camada `.anchored` dos menus); Esc ou clique fora fecham e devolvem o foco. */
export function Popover({ dica, onClose }: { dica: Dica | null; onClose: () => void }) {
  const painel = useRef<HTMLDivElement>(null)
  const idioma = useIdioma()
  const fechar = () => { const alvo = dica?.alvo; onClose(); alvo?.focus() }
  useEscape(dica !== null, fechar, painel)
  useLayoutEffect(() => { if (dica) painel.current?.querySelector<HTMLElement>('button')?.focus() }, [dica])
  if (!dica) return null
  const { style, width } = anchorPlacement(dica.alvo.getBoundingClientRect())
  return createPortal(
    <div className="anchored shown idioma-superficie" {...superficieIdioma(idioma)} onMouseDown={(e) => { if (e.target === e.currentTarget) fechar() }}>
      <div ref={painel} className="anchored-panel bc-popover" role="dialog" aria-label={textoDe(dica.titulo, idioma)} style={{ ...style, maxWidth: Math.min(280, width) }}>
        <Text as="div" size="callout" weight="semibold" tone="text">{textoDe(dica.titulo, idioma)}</Text>
        <Text as="div" size="footnote" tone="secondary" className="selectable">{textoDe(dica.texto, idioma)}</Text>
        <HStack gap={8} justify="flex-end">
          {dica.acao ? <Button kind="secondary" onClick={() => { const a = dica.acao!; fechar(); a.fazer() }}>{textoDe(dica.acao.rotulo, idioma)}</Button> : null}
          <Button kind="secondary" onClick={fechar}>{textoDe(frase('skills.matriz.fechar'), idioma)}</Button>
        </HStack>
      </div>
    </div>,
    document.body,
  )
}

const AVISO_DETALHE: Record<NonNullable<Linha['aviso']>, Frase> = {
  alterada: frase('skills.matriz.avisoDetalhe.alterada'),
  indisponivel: frase('skills.matriz.avisoDetalhe.indisponivel'),
  orfa: frase('skills.matriz.avisoDetalhe.orfa'),
  naoConferida: frase('skills.matriz.avisoDetalhe.naoConferida'),
}

/** Inverso compacto: o número de confirmadas e, com incerteza, um ponto; o texto inteiro na dica, no foco e no popover. */
function Contagem({ linha, onDica }: { linha: Linha; onDica: (alvo: HTMLElement, d: Omit<Dica, 'alvo'>) => void }) {
  const idioma = useIdioma()
  const inverso = textoInverso(linha)
  const texto = textoDe(inverso, idioma)
  return <button className="bc-conta" aria-haspopup="dialog" aria-label={textoDe(frase('skills.matriz.nomeTexto', { nome: linha.nome, texto }), idioma)}
    onClick={(e) => onDica(e.currentTarget, { titulo: cru(linha.nome), texto: inverso })}>
    <span aria-hidden>{linha.ligadas}</span>
    {linha.semConfirmacao ? <span className="bc-conta-ponto" aria-hidden /> : null}
    <span className="bc-dica" aria-hidden>{texto}</span>
  </button>
}

const AVISOS: Record<NonNullable<Linha['aviso']>, Frase> = {
  alterada: frase('skills.conteudo.changed'),
  indisponivel: frase('skills.conteudo.unavailable'),
  orfa: frase('skills.matriz.aviso.orfa'),
  naoConferida: frase('skills.matriz.aviso.naoConferida'),
}

/** O que a faixa de status diz: gesto em andamento, recusado ou sem confirmação; nunca um sucesso presumido. */
function resumo(linhas: Linha[], bots: Bot[], aviso: Frase | null, leitura: Leitura): Frase | null {
  const partes: Frase[] = []
  // `guardado` só existe sem conexão; não diz nada sobre os bots estarem rodando.
  if (leitura === 'guardado') partes.push(frase('skills.matriz.guardadoResumo'))
  if (aviso) partes.push(aviso)
  const salvando = bots.filter((_, i) => linhas.some((l) => l.celulas[i]?.tipo === 'salvando')).map((b) => b.name)
  if (salvando.length) partes.push(plural('skills.matriz.salvandoEm', salvando.length, { bots: salvando.join(', ') }))
  const recusas = linhas.reduce((n, l) => n + l.celulas.filter((c) => c.tipo === 'controle' && c.recusa).length, 0)
  if (recusas) partes.push(plural('skills.matriz.recusas', recusas))
  const incertas = linhas.reduce((n, l) => n + l.celulas.filter((c) => c.tipo === 'incerta').length, 0)
  if (incertas) partes.push(plural('skills.matriz.incertas', incertas))
  return partes.length ? juntas(...partes) : null
}

/** Ícone de estado: um botão com o texto inteiro no nome acessível e na dica; clicar abre o popover. */
function IconeEstado({ icone, tom, titulo, texto, acao, onDica }: {
  icone: string
  tom: 'atencao' | 'neutro'
  titulo: Frase
  texto: Frase
  acao?: Dica['acao']
  onDica: (alvo: HTMLElement, d: Omit<Dica, 'alvo'>) => void
}) {
  const idioma = useIdioma()
  const inteiro = textoDe(frase('skills.matriz.tituloTexto', { titulo, texto }), idioma)
  return <button className={`bc-icone ${tom}`} aria-haspopup="dialog" aria-label={inteiro} title={inteiro}
    onClick={(e) => onDica(e.currentTarget, { titulo, texto, acao })}>
    <Icon name={icone} size={13} weight="medium" />
  </button>
}

function CelulaMatriz({ celula: c, skill, bot, onAlternar, onConferir, onLerDeNovo, onDica }: {
  celula: Celula
  skill: string
  bot: string
  onAlternar: (ligar: boolean) => void
  onConferir: () => void
  onLerDeNovo: () => void
  onDica: (alvo: HTMLElement, d: Omit<Dica, 'alvo'>) => void
}) {
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const onde = frase('skills.matriz.onde', { skill, bot })
  if (c.tipo === 'indisponivel') {
    return <span className="bc-cel"><IconeEstado icone="minus.circle" tom="neutro" titulo={frase('skills.matriz.indisponivelEm', { onde })} texto={juntas(c.motivo, frase('skills.matriz.naoDaParaDizer'))} onDica={onDica} /></span>
  }
  const valor = simNao(c.ligada)
  if (c.tipo === 'texto') {
    return <span className="bc-cel">
      <span className={`bc-valor${c.ligada ? ' sim' : ''}`} aria-label={tx(frase('skills.matriz.celulaLeitura', { skill, bot, valor }))}>{tx(valor)}</span>
      {c.motivo ? <IconeEstado icone="info.circle" tom="neutro" titulo={frase('skills.matriz.soLeituraEm', { onde })} texto={juntas(c.motivo, frase('skills.matriz.confirmado', { valor }))} onDica={onDica} /> : null}
    </span>
  }
  if (c.tipo === 'incerta') {
    return <span className="bc-cel"><IconeEstado icone="exclamationmark.circle" tom="atencao" titulo={frase('skills.matriz.semConfirmacaoEm', { onde })}
      texto={frase('skills.matriz.incertaTexto', { valor })}
      acao={{ rotulo: frase('skills.matriz.conferir'), fazer: onConferir }} onDica={onDica} /></span>
  }
  const salvando = c.tipo === 'salvando'
  const travada = c.tipo === 'controle' ? c.travada : null
  const recusa = c.tipo === 'controle' ? c.recusa : null
  const inerte = salvando || travada !== null
  const motivo = salvando ? frase('skills.matriz.salvandoMotivo', { valor, pedido: simNao(c.pedido) }) : travada
  return <span className="bc-cel">
    {/* O mesmo botão em todos os estados: o foco fica nele enquanto salva; mostra sempre o valor confirmado. */}
    <button className={`bc-sn${c.ligada ? ' sim' : ''}`} role="switch" aria-checked={c.ligada} aria-disabled={inerte || undefined} aria-busy={salvando || undefined}
      aria-label={tx(motivo ? frase('skills.matriz.celulaMotivo', { skill, bot, valor, motivo }) : frase('skills.matriz.celula', { skill, bot, valor }))} title={motivo ? tx(motivo) : undefined}
      onClick={() => { if (!inerte) onAlternar(!c.ligada) }}>
      {tx(valor)}
    </button>
    {salvando ? <span className="bc-icone neutro" title={motivo ? tx(motivo) : undefined} aria-hidden><Spinner size={12} /></span> : null}
    {recusa ? <IconeEstado icone="exclamationmark.triangle" tom="atencao" titulo={frase('skills.matriz.naoMudouEm', { onde })} texto={juntas(recusa, frase('skills.matriz.continua', { valor }))}
      acao={{ rotulo: frase('skills.matriz.lerDeNovo'), fazer: onLerDeNovo }} onDica={onDica} /> : null}
    {travada === TEXTOS_MATRIZ.ligarAlterada ? <IconeEstado icone="info.circle" tom="neutro" titulo={frase('skills.matriz.soDesligarEm', { onde })} texto={travada} onDica={onDica} /> : null}
  </span>
}
