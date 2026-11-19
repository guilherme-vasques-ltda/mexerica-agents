// MX-95 (a partir da proposta MX-84, aprovada pelo dono em 11/11/2026): Bot → Ferramentas em três abas, uma por
// seção que já existia (Conectores, Logins, Skills). Cada aba tem busca sempre à mostra e páginas de 15 no rodapé.
// Regras e contratos não mudam: a mesma leitura (`usePluginsDoBot`, uma vez para as três), as mesmas intenções
// (`save('connectors' | 'logins')`, `saveSkill`) e os mesmos erros. Trocar de aba, buscar ou paginar não grava nada:
// a navegação mora num reducer à parte (`navegar`, em lista.ts) que nem enxerga a seleção do bot.
import { memo, useCallback, useDeferredValue, useId, useLayoutEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react'
import type { Bot } from '@shared/models'
import { Button, CardSection, IconButton, SearchField, Switch } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { SwitchRow } from '../../bots/parts'
import { loginTitle } from '../../marketplace/market-models'
import { FormSheet } from '../../marketplace/pieces'
import { SkillSheet, type SkillRef } from '../../marketplace/SkillSheet'
import { abaHorizontalPeloTeclado } from '../bot-settings/abas'
import type { SaveSettings, SaveSkill, SettingsFeedback } from '../bot-settings/CurrentSettingsSections'
import { IndiceDeSkills, usePluginsDoBot, type PluginsDoBot } from '../bot-settings/IndiceDeSkills'
import { textoDaMensagem } from '../bot-settings/mensagem'
import { origemDaFonte } from '../catalogo-skills/consumidores'
import { LogoConector } from '../extensoes/LogoConector'
import { ContasDoBot } from '../mercado/ContasDoBot'
import { ABAS, alternar, filtrar, indexar, navegacaoInicial, navegar, numerosDaPaginacao, paginar, type Aba, type ItemDeFerramenta, type Navegacao } from './lista'
import './bot-ferramentas.css'

const TEXTOS = {
  connectors: { titulo: 'bots.editor.conectores', vazio: 'bots.ajustes.conectoresVazio', buscar: 'bots.ajustes.abas.buscar.conectores', erro: 'bots.ajustes.erro.conectores' },
  logins: { titulo: 'bots.editor.logins', vazio: 'bots.ajustes.loginsVazio', buscar: 'bots.ajustes.abas.buscar.logins', erro: 'bots.ajustes.erro.logins' },
  skills: { titulo: 'bots.editor.skills', vazio: 'bots.ajustes.skillsVazio', buscar: 'bots.ajustes.abas.buscar.skills', erro: 'bots.ajustes.erro.skills' },
} as const

/** A aba em que cada bot ficou, enquanto o app está aberto (a seção das Configurações também só vive na tela). */
const abaPorBot = new Map<string, Aba>()

type Item = ItemDeFerramenta & { icon?: ReactNode }
/** Login sem rótulo: o título já é "site · usuário"; a segunda linha não repete. */
const semRepetir = (titulo: string, detalhe: string) => (detalhe === titulo ? '' : detalhe)

interface Props { bot: Bot; save: SaveSettings; saveSkill: SaveSkill; feedback: SettingsFeedback; disabled?: boolean }

/** A entrada: lê do Hub uma vez para as três abas e entrega à tela. */
export function FerramentasEmAbas(props: Props) {
  const store = useStore()
  const plugins = usePluginsDoBot(store, props.bot)
  const online = store.connection.kind === 'online'
  const sobreSkill = useCallback((item: ItemDeFerramenta): SkillRef => ({ id: item.id, name: item.name, description: item.detail, origem: origemDaFonte(store) }), [store])
  const indice = plugins ? <IndiceDeSkills store={store} bot={props.bot} leitura={plugins.indice} /> : null
  return (
    <>
      {/* Marketplace (fatia 1b): a tabela "Contas que o bot usa", acima das abas. */}
      <ContasDoBot bot={props.bot} />
      <PainelDeFerramentas {...props} plugins={plugins} online={online} indice={indice} sobreSkill={sobreSkill} />
    </>
  )
}

/**
 * A tela, sem Hub: recebe as listas prontas. `disabled` (offline, sem permissão ou um salvamento em curso) desliga SÓ
 * os interruptores; abas, busca e páginas são navegação e continuam (a seção fica fora do `fieldset disabled`).
 * `inicial` existe para os testes renderizarem uma aba, busca ou página escolhida.
 */
export function PainelDeFerramentas({ bot, save, saveSkill, feedback, disabled = false, plugins, online, indice, sobreSkill, inicial }: Props & {
  plugins: PluginsDoBot | null; online: boolean; indice: ReactNode; sobreSkill: (item: ItemDeFerramenta) => SkillRef; inicial?: Navegacao
}) {
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  const [nav, despachar] = useReducer(navegar, null, () => inicial ?? navegacaoInicial(abaPorBot.get(bot.id)))
  const [skill, setSkill] = useState<SkillRef | null>(null)
  const botoes = useRef<Partial<Record<Aba, HTMLButtonElement | null>>>({})
  const escolher = (proxima: Aba) => { abaPorBot.set(bot.id, proxima); despachar({ tipo: 'aba', aba: proxima }) }

  // As listas viram itens uma vez por leitura do Hub, não por tecla nem por troca de aba.
  const conectores = useMemo<Item[]>(() => (plugins?.connectors ?? []).map((c) => ({
    // O nome vai inteiro, como o Hub manda: com mais de uma conta do mesmo app ele já traz o rótulo ("Stripe · Loja A").
    id: c.id, name: c.name, detail: c.description || c.command || c.url || '', icon: <LogoConector conector={c} size={30} />,
  })), [plugins?.connectors])
  const logins = useMemo<Item[]>(() => (plugins?.logins ?? []).map((l) => ({ id: l.id, name: loginTitle(l), detail: semRepetir(loginTitle(l), l.username ? `${l.site} · ${l.username}` : l.site) })), [plugins?.logins])
  const skills = useMemo<Item[]>(() => (plugins?.skills ?? []).map((s) => ({ id: s.id, name: s.name, detail: s.description })), [plugins?.skills])

  // Os gestos leem o estado mais novo por referência: as funções não mudam e as linhas em memo não redesenham.
  // Cada gesto parte da seleção INTEIRA do bot e muda só o id tocado (`alternar`).
  const atual = useRef({ bot, save, saveSkill, plugins, sobreSkill })
  atual.current = { bot, save, saveSkill, plugins, sobreSkill }
  const ligarConector = useCallback((id: string, on: boolean) => { const a = atual.current; a.save('connectors', { connectors: alternar(a.bot.connectors ?? [], id, on) }) }, [])
  const ligarLogin = useCallback((id: string, on: boolean) => { const a = atual.current; a.save('logins', { logins: alternar(a.bot.logins ?? [], id, on) }) }, [])
  const ligarSkill = useCallback((id: string, on: boolean) => { const a = atual.current; const s = a.plugins?.skills.find((x) => x.id === id); if (s) a.saveSkill(s, on) }, [])
  const abrirSkill = useCallback((item: ItemDeFerramenta) => setSkill(atual.current.sobreSkill(item)), [])
  const buscar = useCallback((texto: string) => despachar({ tipo: 'busca', texto }), [])
  const irPara = useCallback((pagina: number) => despachar({ tipo: 'pagina', pagina }), [])

  if (!plugins) return <p className="bf-estado idioma-superficie" role="status" {...idioma}>{t(online ? 'bots.ajustes.carregando' : 'bots.ajustes.abas.semConexao')}</p>

  const paineis: Record<Aba, { itens: Item[]; selecao: string[]; ligar: (id: string, on: boolean) => void; rodape: string; erroLista?: string }> = {
    connectors: { itens: conectores, selecao: bot.connectors ?? [], ligar: ligarConector, rodape: t('bots.editor.conectoresRodape'), erroLista: plugins.errors?.connectors },
    logins: { itens: logins, selecao: bot.logins ?? [], ligar: ligarLogin, rodape: t('bots.ajustes.loginsRodape'), erroLista: plugins.errors?.logins },
    skills: { itens: skills, selecao: bot.skills ?? [], ligar: ligarSkill, rodape: t('bots.ajustes.indice.sobre'), erroLista: plugins.errors?.skills },
  }
  const uid = `bot-ferramentas-${bot.id}`
  return <div className="idioma-superficie bf" {...idioma}>
    {online ? null : <p className="bf-aviso" role="status">{t('bots.ajustes.abas.semConexaoNavega')}</p>}
    <div className="tab-choice bf-abas" role="tablist" aria-label={t('bots.ajustes.abas.rotulo')}>
      {ABAS.map((id) => {
        const p = paineis[id]
        const comErro = !!p.erroLista || !!feedback.errors[id]
        const escolhida = id === nav.aba
        return <button key={id} type="button" role="tab" id={`${uid}-aba-${id}`} aria-controls={`${uid}-painel-${id}`} aria-selected={escolhida} tabIndex={escolhida ? 0 : -1}
          ref={(el) => { botoes.current[id] = el }} className={`tab-choice-option bf-aba${escolhida ? ' on' : ''}`}
          aria-label={p.erroLista ? undefined : t('bots.ajustes.abas.contagem', { nome: t(TEXTOS[id].titulo), n: p.itens.length })}
          onClick={() => escolher(id)} onKeyDown={(e) => {
            const proxima = abaHorizontalPeloTeclado(ABAS, id, e.key, idioma.dir)
            if (!proxima) return
            e.preventDefault()
            escolher(proxima)
            botoes.current[proxima]?.focus({ preventScroll: true })
          }}>
          {t(TEXTOS[id].titulo)}
          {p.erroLista ? null : <span className="bf-aba-n" aria-hidden="true">{p.itens.length}</span>}
          {comErro ? <span className="bot-settings-tab-error" aria-hidden="true" /> : null}
        </button>
      })}
    </div>
    {/* As três abas ficam montadas: trocar não refaz a leitura do Hub nem perde a busca de cada uma. */}
    {ABAS.map((id) => {
      const p = paineis[id]
      const erro = feedback.errors[id]
      return <div key={id} role="tabpanel" id={`${uid}-painel-${id}`} aria-labelledby={`${uid}-aba-${id}`} hidden={id !== nav.aba} className="bf-painel">
        {p.erroLista ? <div className="bot-settings-list-error" role="alert">
          <span>{t(TEXTOS[id].erro, { erro: p.erroLista })}</span>
          {plugins.retry ? <Button kind="secondary" onClick={plugins.retry}>{t('bots.ajustes.tentarDeNovo')}</Button> : null}
        </div> : <ListaDaAba nome={t(TEXTOS[id].titulo)} rotuloBusca={t(TEXTOS[id].buscar)} vazio={t(TEXTOS[id].vazio)} rodape={p.rodape}
          itens={p.itens} selecao={p.selecao} travado={disabled} busca={nav.busca[id]} pagina={nav.pagina[id]} onBuscar={buscar} onPagina={irPara}
          onLigar={p.ligar} onSobre={id === 'skills' ? abrirSkill : undefined} acessorio={id === 'skills' ? indice : null} />}
        {erro ? <p className="bot-settings-control-error" role="alert">{textoDaMensagem(t, erro)}</p> : null}
      </div>
    })}
    <FormSheet language={idioma} open={skill !== null} onClose={() => setSkill(null)}>{skill ? <SkillSheet key={skill.id} skill={skill} /> : null}</FormSheet>
  </div>
}

/** Uma aba: busca sempre à mostra, no conjunto todo; 15 por página, com as páginas no rodapé quando passam de uma. */
const ListaDaAba = memo(function ListaDaAba({ nome, rotuloBusca, vazio, rodape, itens, selecao, travado, busca, pagina, onBuscar, onPagina, onLigar, onSobre, acessorio }: {
  nome: string; rotuloBusca: string; vazio: string; rodape: string; itens: Item[]; selecao: string[]; travado: boolean
  busca: string; pagina: number; onBuscar: (texto: string) => void; onPagina: (pagina: number) => void
  onLigar: (id: string, on: boolean) => void; onSobre?: (item: ItemDeFerramenta) => void; acessorio?: ReactNode
}) {
  const t = useT()
  const barra = useRef<HTMLDivElement>(null)
  // Trocar de página leva o olho de volta ao começo da lista (as páginas ficam no fim dela).
  const irPara = (p: number) => { onPagina(p); requestAnimationFrame(() => barra.current?.scrollIntoView({ block: 'nearest' })) }
  // A digitação nunca espera a lista: o campo responde na hora e o filtro roda com o valor adiado.
  const buscaAdiada = useDeferredValue(busca)
  const indice = useMemo(() => indexar(itens), [itens])
  const achados = useMemo(() => filtrar(indice, buscaAdiada), [indice, buscaAdiada])
  const fatia = useMemo(() => paginar(achados, pagina), [achados, pagina])
  const ligados = useMemo(() => new Set(selecao), [selecao])
  const nLigados = useMemo(() => itens.reduce((n, i) => n + (ligados.has(i.id) ? 1 : 0), 0), [itens, ligados])
  return <>
    <div className="bf-barra" ref={barra}>
      <SearchField value={busca} onChange={onBuscar} prompt={rotuloBusca} clearLabel={t('bots.ajustes.abas.limparBusca')} inputProps={{ 'aria-label': rotuloBusca }} />
      {itens.length ? <span className="bf-ligados">{t('bots.ajustes.abas.ligados', { ligados: nLigados, total: itens.length })}</span> : null}
    </div>
    {acessorio ? <div className="bf-acessorio">{acessorio}</div> : null}
    {!itens.length ? <p className="bf-vazio" role="status">{vazio}</p>
      : fatia.total === 0 ? <p className="bf-vazio" role="status">{t('bots.ajustes.abas.semResultado', { busca: buscaAdiada.trim() })}</p>
        : <CardSection footer={rodape}>
          {fatia.itens.map((item) => onSobre
            ? <LinhaDeSkill key={item.id} item={item} ligado={ligados.has(item.id)} travado={travado} onLigar={onLigar} onSobre={onSobre} />
            : <Linha key={item.id} item={item} ligado={ligados.has(item.id)} travado={travado} onLigar={onLigar} />)}
        </CardSection>}
    {fatia.paginas > 1 ? <nav className="bf-paginas" aria-label={t('bots.ajustes.abas.paginacao', { nome })}>
      <span className="bf-faixa" role="status">{t('bots.ajustes.abas.faixa', { de: fatia.de, ate: fatia.ate, total: fatia.total })}</span>
      {/* ‹ e › são espelhados pelo próprio texto em árabe (bidi): "anterior" aponta para a direita lá. */}
      <span className="bf-numeros">
        <button type="button" className="bf-num bf-seta" disabled={fatia.pagina <= 1} aria-label={t('bots.ajustes.abas.anterior')} onClick={() => irPara(fatia.pagina - 1)}>‹</button>
        {numerosDaPaginacao(fatia.pagina, fatia.paginas).map((n, i) => n === null
          ? <span key={`r${i}`} className="bf-reticencias" aria-hidden="true">…</span>
          : <button key={n} type="button" className={`bf-num${n === fatia.pagina ? ' on' : ''}`} aria-current={n === fatia.pagina ? 'page' : undefined}
            aria-label={t('bots.ajustes.abas.irPara', { pagina: n })} onClick={() => irPara(n)}>{n}</button>)}
        <button type="button" className="bf-num bf-seta" disabled={fatia.pagina >= fatia.paginas} aria-label={t('bots.ajustes.abas.proxima')} onClick={() => irPara(fatia.pagina + 1)}>›</button>
      </span>
    </nav> : null}
  </>
})

const Linha = memo(function Linha({ item, ligado, travado, onLigar }: { item: Item; ligado: boolean; travado: boolean; onLigar: (id: string, on: boolean) => void }) {
  return <SwitchRow title={item.name} detail={item.detail || null} icon={item.icon} on={ligado} disabled={travado} onChange={(on) => onLigar(item.id, on)} />
})

/**
 * Skill: a descrição fica em 2 linhas; "ver mais" abre na própria linha. O interruptor é um controle à parte (botão
 * dentro de botão não vale), então o "ver mais" nunca liga nem desliga a skill.
 */
const LinhaDeSkill = memo(function LinhaDeSkill({ item, ligado, travado, onLigar, onSobre }: { item: Item; ligado: boolean; travado: boolean; onLigar: (id: string, on: boolean) => void; onSobre: (item: ItemDeFerramenta) => void }) {
  const t = useT()
  const id = useId()
  const texto = useRef<HTMLParagraphElement>(null)
  const [aberta, setAberta] = useState(false)
  const [cortada, setCortada] = useState(false)
  useLayoutEffect(() => {
    const el = texto.current
    if (!el || aberta) return
    const medir = () => setCortada(el.scrollHeight > el.clientHeight + 1)
    medir()
    const obs = new ResizeObserver(medir)
    obs.observe(el)
    return () => obs.disconnect()
  }, [aberta, item.detail])
  return <div className="bf-linha">
    <div className="bf-texto">
      <span className="bf-nome">{item.name}</span>
      {item.detail ? <p ref={texto} id={`${id}-d`} className={`bf-desc${aberta ? ' aberta' : ''}`}>{item.detail}</p> : null}
      {item.detail && (cortada || aberta) ? <button type="button" className="bf-vermais" aria-expanded={aberta} aria-controls={`${id}-d`} onClick={() => setAberta(!aberta)}>
        {t(aberta ? 'bots.ajustes.abas.verMenos' : 'bots.ajustes.abas.verMais')}
      </button> : null}
    </div>
    <IconButton title={t('bots.editor.sobre', { nome: item.name })} icon="info.circle" onClick={() => onSobre(item)} />
    <Switch on={ligado} label={item.name} disabled={travado} onChange={(on) => onLigar(item.id, on)} />
  </div>
})
