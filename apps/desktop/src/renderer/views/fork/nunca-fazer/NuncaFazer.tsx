// MX-111 (fase 1, só orientação): a lista "Nunca fazer" no bot (Comportamento, depois de Autonomia) e em
// Ajustes › Nunca fazer (só o Space ativo). Sem bloqueio real: a linha "É orientação, não garantia" e os desligamentos
// que já existem (computador, navegador, conector inteiro). Leitura e gravação no modelo (modelo.ts).
import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import type { Bot } from '@shared/models'
import { Button, CardSection, ChoicePicker, DropdownMenu } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { useModel } from '../../../lib/observable'
import { semParametros } from '../../settings/rotulo'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { settingsScreen } from '../ajustes/tela'
import { computadorDaPagina, contar, falhaFatal, falhaSemModelo, LIMITE_ITENS, LIMITE_TEXTO, NuncaFazerStore, rascunhosOrfaos, spaceDaPagina, textoValido, type Falha, type ItemNuncaFazer, type ListaNuncaFazer } from './modelo'
import './nunca-fazer.css'

function abrirEmAjustes() { settingsScreen.show('never-do') }

const nomeDoSpace = (store: BotStore, spaceId: string) => store.spaces?.spaces.find((s) => s.id === spaceId)?.name ?? ''

/** Um modelo por conexão e escopo; relê ao voltar a conexão. */
function useLista(store: BotStore, spaceId: string | null, botId: string | null) {
  const host = useModel(store)
  const client = host.connectedClient
  const hub = store.computer.id
  const model = useMemo(() => spaceId ? new NuncaFazerStore(() => store.connectedClient, { spaceId, botId, hub }) : null, [store, spaceId, botId, hub])
  useModel(model)
  useEffect(() => { if (model && client) void model.carregar() }, [model, client])
  // UI706-01: sem evento do Hub para esta lista, relê quando outro modelo grava (modelo.ts), quando a janela
  // volta ao foco e quando Ajustes fecha (o editor do bot fica montado embaixo e volta à vista).
  useEffect(() => {
    if (!model) return
    const desligar = model.ativar()
    const focar = () => { if (document.visibilityState === 'visible') model.invalidar() }
    let aberto = settingsScreen.open
    const ajustes = () => { if (aberto && !settingsScreen.open) model.invalidar(); aberto = settingsScreen.open }
    const sair = settingsScreen.subscribe(ajustes)
    window.addEventListener('focus', focar)
    document.addEventListener('visibilitychange', focar)
    return () => { desligar(); sair(); window.removeEventListener('focus', focar); document.removeEventListener('visibilitychange', focar) }
  }, [model])
  return { model, online: client !== null }
}

function Aviso({ falha, onTentar }: { falha: Falha; onTentar?: () => void }) {
  const t = useT()
  return <div className="nf-aviso" role={falha === 'semConexao' || falha === 'hubAntigo' ? 'status' : 'alert'}>
    <Icon name={falha === 'semConexao' ? 'wifi.slash' : 'exclamationmark.triangle'} size={12} className="nf-aviso-icone" />
    <span className="nf-aviso-texto">{semParametros(t)(`nuncaFazer.falha.${falha}`)}</span>
    {onTentar ? <button type="button" className="nf-link" onClick={onTentar}>{t('nuncaFazer.tentar')}</button> : null}
  </div>
}

function Campo({ valor, onValor, onEnviar, travado, rotulo, autoFocus }: { valor: string; onValor: (v: string) => void; onEnviar: () => void; travado: boolean; rotulo: string; autoFocus?: boolean }) {
  const t = useT()
  const n = contar(valor)
  return <div className="nf-campo">
    <input className="field-box" dir="auto" value={valor} disabled={travado} autoFocus={autoFocus} aria-label={rotulo} placeholder={t('nuncaFazer.campoExemplo')}
      onChange={(e) => onValor(e.target.value)} onKeyDown={(e) => {
        // UI706-02: Enter que confirma a composição (CJK, acentos) não envia; como o resto do app.
        if (e.key !== 'Enter' || e.nativeEvent.isComposing || e.keyCode === 229 || !textoValido(valor)) return
        e.preventDefault()
        onEnviar()
      }} />
    <span className={`nf-caracteres${n > LIMITE_TEXTO ? ' excesso' : ''}`} dir="ltr" aria-live="polite">{t('nuncaFazer.caracteres', { n, max: LIMITE_TEXTO })}</span>
  </div>
}

/** Rascunhos de edição por id do item. Ficam na lista, não na linha (TEC706-01): se o item some (removido em
 * outra janela, conflito, item_not_found), o texto digitado continua à vista em vez de desmontar com a linha. */
type Rascunhos = Readonly<Record<string, string>>

function Linha({ item, model, travado, primeiro, ultimo, rascunho, setRascunho }: { item: ItemNuncaFazer; model: NuncaFazerStore; travado: boolean; primeiro: boolean; ultimo: boolean; rascunho: string | null; setRascunho: (texto: string | null) => void }) {
  const t = useT()
  const idioma = useIdioma()
  // P3: ao salvar ou cancelar a edição, o foco volta ao botão "Mais opções" da linha (não cai no corpo).
  const linha = useRef<HTMLDivElement>(null)
  const voltarFoco = useRef(false)
  useEffect(() => {
    if (rascunho !== null || travado || !voltarFoco.current) return
    voltarFoco.current = false
    linha.current?.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')?.focus()
  }, [rascunho, travado])
  const sair = () => { voltarFoco.current = true; setRascunho(null) }
  const salvar = async () => { if (rascunho !== null && await model.editar(item.id, rascunho)) sair() }
  if (rascunho !== null) return <div className="nf-item editando">
    <Campo valor={rascunho} onValor={setRascunho} onEnviar={() => void salvar()} travado={travado} rotulo={t('nuncaFazer.editar')} autoFocus />
    <div className="nf-acoes">
      <Button kind="secondary" onClick={sair}>{t('nuncaFazer.cancelar')}</Button>
      <Button disabled={travado || !textoValido(rascunho) || rascunho.trim() === item.texto} onClick={() => void salvar()}>{t('nuncaFazer.salvar')}</Button>
    </div>
  </div>
  return <div className="nf-item" ref={linha}>
    <span className="nf-icone" aria-hidden><Icon name="hand.raised" size={13} /></span>
    <span className="nf-texto" dir="auto">{item.texto}</span>
    <DropdownMenu language={superficieIdioma(idioma)} title={t('nuncaFazer.mais')} disabled={travado} items={() => [
      { title: t('nuncaFazer.editar'), icon: 'pencil', action: () => setRascunho(item.texto) },
      { title: t('nuncaFazer.subir'), icon: 'arrow.up', disabled: primeiro, action: () => void model.mover(item.id, -1) },
      { title: t('nuncaFazer.descer'), icon: 'arrow.down', disabled: ultimo, action: () => void model.mover(item.id, 1) },
      { title: t('nuncaFazer.remover'), icon: 'trash', destructive: true, action: () => void model.remover(item.id) },
    ]}><span className="nf-mais"><Icon name="ellipsis" size={13} /></span></DropdownMenu>
  </div>
}

const EXEMPLOS: ChaveTexto[] = ['nuncaFazer.exemplo.email', 'nuncaFazer.exemplo.apagar', 'nuncaFazer.exemplo.pagar']

/** A lista que esta tela edita: itens, campo de adicionar com contador e exemplos na lista vazia. */
function Editavel({ model, online, vazio, guardarComo = 'avulso' }: { model: NuncaFazerStore; online: boolean; vazio: ChaveTexto; guardarComo?: string }) {
  const t = useT()
  // O texto do novo item e os rascunhos de edição ficam no pai (estado-pagina.ts): a página volta com eles.
  const [novo, setNovo] = useEstadoDaPagina(`nunca-fazer/${guardarComo}/novo`, '')
  const [rascunhos, setRascunhos] = useEstadoDaPagina<Rascunhos>(`nunca-fazer/${guardarComo}/rascunhos`, {})
  const travado = !online || model.travado
  const itens = model.editaveis
  const adicionar = async () => { if (await model.adicionar(novo)) setNovo('') }
  const rascunho = (id: string, texto: string | null) => setRascunhos((r) => {
    const { [id]: _, ...resto } = r
    return texto === null ? resto : { ...resto, [id]: texto }
  })
  // Rascunhos de itens que não estão mais na lista (só com a lista lida de novo, nunca durante a leitura).
  const orfaos = rascunhosOrfaos(rascunhos, itens, model.lendo)
  const recriar = async (id: string, texto: string) => { if (await model.adicionar(texto)) rascunho(id, null) }
  return <div className="nf-grupo">
    {itens.length ? <div className="nf-lista">
      {itens.map((item, i) => <Linha key={item.id} item={item} model={model} travado={travado} primeiro={i === 0} ultimo={i === itens.length - 1}
        rascunho={rascunhos[item.id] ?? null} setRascunho={(texto) => rascunho(item.id, texto)} />)}
    </div> : <div className="nf-vazio">
      <p>{semParametros(t)(vazio)}</p>
      <div className="nf-exemplos" role="group" aria-label={t('nuncaFazer.exemplos')}>
        {EXEMPLOS.map((e) => <button type="button" key={e} className="nf-exemplo" disabled={travado} onClick={() => setNovo(semParametros(t)(e))}><Icon name="plus" size={10} />{semParametros(t)(e)}</button>)}
      </div>
    </div>}
    {orfaos.map(([id, texto]) => <div key={id} className="nf-item editando orfao" role="group" aria-label={t('nuncaFazer.rascunhoOrfao')}>
      <p className="nf-nota">{t('nuncaFazer.rascunhoOrfao')}</p>
      <Campo valor={texto} onValor={(v) => rascunho(id, v)} onEnviar={() => void recriar(id, texto)} travado={travado} rotulo={t('nuncaFazer.rascunhoOrfao')} />
      <div className="nf-acoes">
        <Button kind="secondary" onClick={() => rascunho(id, null)}>{t('nuncaFazer.descartar')}</Button>
        <Button disabled={travado || model.cheia || !textoValido(texto)} onClick={() => void recriar(id, texto)}>{t('nuncaFazer.adicionarComoNovo')}</Button>
      </div>
    </div>)}
    {model.cheia ? <p className="nf-nota">{t('nuncaFazer.cheia')}</p> : <div className="nf-adicionar">
      <Campo valor={novo} onValor={setNovo} onEnviar={() => void adicionar()} travado={travado} rotulo={t('nuncaFazer.campo')} />
      <Button disabled={travado || !textoValido(novo)} onClick={() => void adicionar()}>{t('nuncaFazer.adicionar')}</Button>
    </div>}
    <span className="nf-contagem">{t('nuncaFazer.contagem', { n: itens.length, max: LIMITE_ITENS })}</span>
  </div>
}

function Leitura({ itens }: { itens: ItemNuncaFazer[] }) {
  return <div className="nf-lista">{itens.map((item) => <div key={item.id} className="nf-item herdado">
    <span className="nf-icone" aria-hidden><Icon name="hand.raised" size={13} /></span>
    <span className="nf-texto" dir="auto">{item.texto}</span>
  </div>)}</div>
}

/** Aviso, "Lendo…" ou a lista. Sem conexão a lista lida continua à vista, travada; falha que esconde a lista mostra só o aviso. */
function Corpo({ model, online, children }: { model: NuncaFazerStore | null; online: boolean; children: (model: NuncaFazerStore, lista: ListaNuncaFazer) => ReactNode }) {
  const t = useT()
  const falha: Falha | null = !model ? falhaSemModelo(online) : !online ? 'semConexao' : model.falha
  const lista = model?.lista ?? null
  if (!model || !lista || falhaFatal(falha)) {
    if (!falha || model?.lendo) return <p className="nf-nota">{t('nuncaFazer.lendo')}</p>
    return <Aviso falha={falha ?? 'leitura'} onTentar={falha === 'leitura' && model ? () => void model.carregar() : undefined} />
  }
  // TEC706-02: com a lista possivelmente velha a escrita fica travada; o aviso oferece reler em vez de liberar.
  const reler = online && model.desatualizada && !model.lendo ? () => void model.carregar() : undefined
  return <div className="nf-corpo">{falha || model.desatualizada ? <Aviso falha={falha ?? 'leitura'} onTentar={reler} /> : null}{children(model, lista)}</div>
}

/** Bot › Comportamento: primeiro os do Space (só leitura), depois os só deste bot. */
export function NuncaFazerBot({ store, bot, onFerramentas }: { store: BotStore; bot: Bot; onFerramentas: () => void }) {
  const t = useT()
  const spaceId = bot.spaceId ?? store.spaceId ?? store.spaces?.personalSpaceId ?? null
  const { model, online } = useLista(store, spaceId, bot.id)
  const rodape = <>
    <span>{t('nuncaFazer.ajuda')}</span>
    <span className="nf-desligar">{t('nuncaFazer.desligar')} <button type="button" className="nf-link" onClick={onFerramentas}>{t('nuncaFazer.irFerramentas')}<Icon name="arrow.right" size={10} /></button></span>
  </>
  return <div className="nf">
    <CardSection title={t('nuncaFazer.titulo')} footer={rodape}>
      <Corpo model={model} online={online}>{(model, lista) => <>
        {lista.space.length ? <div className="nf-grupo">
          <div className="nf-grupo-cabeca">
            <span className="nf-grupo-titulo">{t('nuncaFazer.herdados', { space: nomeDoSpace(store, model.escopo.spaceId) })}</span>
            <button type="button" className="nf-link" onClick={abrirEmAjustes}>{t('nuncaFazer.editarEmAjustes')}<Icon name="arrow.up.right.square" size={10} /></button>
          </div>
          <span className="nf-grupo-ajuda">{t('nuncaFazer.herdadosAjuda')}</span>
          <Leitura itens={lista.space} />
        </div> : null}
        <div className="nf-grupo">
          {lista.space.length ? <div className="nf-grupo-cabeca"><span className="nf-grupo-titulo">{t('nuncaFazer.deste')}</span></div> : null}
          <Editavel key={`${model.escopo.spaceId}/${model.escopo.botId}`} model={model} online={online} vazio="nuncaFazer.vazio" />
        </div>
      </>}</Corpo>
    </CardSection>
  </div>
}

/** Ajustes › Nunca fazer: a lista do Space, que todo bot dele lê. */
export function NuncaFazerPagina() {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const ativo = app.spaceMirrors.active
  const [escolhido, setComputador] = useEstadoDaPagina('nunca-fazer/computador', () => app.selectedStore?.computer.id ?? (app.computers.length === 1 ? app.computers[0]!.id : ''))
  // Com Space ativo o computador é o dele (sem escolha própria); só com o Pessoal aberto a pessoa escolhe o computador.
  const computador = computadorDaPagina(ativo, escolhido)
  const store = computador ? app.spaceMirrors.root(computador) : null
  return <div className="nf nf-pagina idioma-superficie" {...superficieIdioma(idioma)}>
    <p className="nf-lead">{t('nuncaFazer.pagina.lead')}</p>
    {!ativo && app.computers.length > 1 && <label className="nf-escolha"><span>{t('memoria.computador')}</span>
      <ChoicePicker language={superficieIdioma(idioma)} selection={computador} options={[{ id: '', label: t('memoria.computador.escolha') }, ...app.computers.map((c) => ({ id: c.id, label: c.name }))]} onChange={setComputador} /></label>}
    {store ? <PaginaDoHub key={`${computador}/${ativo?.spaceId ?? ''}`} store={store} /> : <p className="nf-nota">{t('memoria.computador.escolha')}</p>}
  </div>
}

function PaginaDoHub({ store: fonte }: { store: BotStore }) {
  const t = useT()
  const app = useApp()
  const store = useModel(fonte)
  const spaces = store.spaces?.spaces ?? []
  // Só o Space ativo no seletor do topo; para ver outro, a pessoa troca lá.
  const spaceId = spaceDaPagina(app.spaceMirrors.active, store.computer.id, store.hello?.hostId, store.spaces?.personalSpaceId)
  const { model, online } = useLista(store, spaceId, null)
  return <CardSection title={spaceId && spaces.length > 1 ? nomeDoSpace(store, spaceId) : null} footer={<><span>{t('nuncaFazer.ajuda')}</span><span className="nf-desligar">{t('nuncaFazer.desligarAjustes')}</span></>}>
    <Corpo model={model} online={online}>{(model) => <Editavel key={model.escopo.spaceId} guardarComo={`${store.computer.id}/${model.escopo.spaceId}`} model={model} online={online} vazio="nuncaFazer.vazioSpace" />}</Corpo>
  </CardSection>
}
