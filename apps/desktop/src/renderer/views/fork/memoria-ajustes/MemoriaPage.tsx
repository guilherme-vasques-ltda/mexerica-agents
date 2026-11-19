import { useEffect, useMemo, useRef, useSyncExternalStore, type KeyboardEvent } from 'react'
import { Button, ChoicePicker } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { radioArrowTarget, radioTabStop } from '../../../lib/radio-group'
import { useModel } from '../../../lib/observable'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { BackgroundStore, accountOptions, savedAccountId, validDraft, type AccountWords, type BackgroundState } from '../bot-de-fundo/model'
import { LEARNINGS, LearningStore, modelFamily, modelNotices, modelPatch, type Learning } from './model'
import { CAMPOS, SobreVoceStore, identidade, type Campo } from './sobre-voce'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import './memoria-ajustes.css'

const STATE = { connected: 'memoria.modelo.conta.conectada', unverified: 'memoria.modelo.conta.naoConfirmada', unavailable: 'memoria.modelo.conta.indisponivel', disconnected: 'memoria.modelo.conta.desconectada' } as const
/** Os mesmos nomes e estados de Contas de IA, traduzidos. */
const accountWords = (t: Tradutor): AccountWords => ({
  legacy: t('memoria.modelo.conta.perfilAntigo'),
  subscription: t('memoria.modelo.conta.assinatura'),
  refused: t('memoria.modelo.conta.naoFuncionaMemoria'),
  state: (state) => t(Object.hasOwn(STATE, state) ? STATE[state as keyof typeof STATE] : 'memoria.modelo.conta.desconectada'),
})
const FAMILY = { haiku: 'memoria.modelo.haiku', sonnet: 'memoria.modelo.sonnet', opus: 'memoria.modelo.opus' } as const
const scopeOf = (host: BotStore) => `${host.hello?.hostId ?? ''}/${host.principalEpoch}/${host.connectedClient ? 'connected' : 'unavailable'}`

/** D389: Ajustes → Memória, um cartão. Sem computador conectado nada é lido nem gravado. */
export function MemoriaPage() {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const [computer, setComputer] = useEstadoDaPagina('memoria/computador', () => app.selectedStore?.computer.id ?? (app.computers.length === 1 ? app.computers[0]!.id : ''))
  const host = computer ? app.store(computer) : null
  return <div className="memoria-ajustes idioma-superficie" {...superficieIdioma(idioma)}>
    {app.computers.length > 1 && <label className="memoria-computador"><span>{t('memoria.computador')}</span>
      <ChoicePicker language={superficieIdioma(idioma)} selection={computer} options={[{ id: '', label: t('memoria.computador.escolha') }, ...app.computers.map((c) => ({ id: c.id, label: c.name }))]} onChange={setComputer} /></label>}
    {host ? <><SemConexao host={host} /><SobreVoceCard key={`sv-${computer}`} host={host} /><MemoriaCard key={computer} host={host} /></> : <p className="memoria-nota">{t('memoria.computador.escolha')}</p>}
  </div>
}

function MemoriaCard({ host: source }: { host: BotStore }) {
  const t = useT()
  const host = useModel(source)
  const scope = scopeOf(host)
  const client = host.connectedClient
  const online = client !== null
  const connect = async () => {
    const connected = host.connectedClient
    if (!connected) throw new Error('memory_connection_unavailable')
    return connected
  }
  // A mesma leitura/gravação da tela Bot de fundo: nenhum segundo lugar guarda o modelo da memória.
  const background = useMemo(() => new BackgroundStore(connect, () => scopeOf(host), () => host.connectedClient), [host, scope, client])
  const learning = useModel(useMemo(() => new LearningStore(connect), [host, scope, client]))
  const state = useSyncExternalStore(background.subscribe, background.getSnapshot)
  useEffect(() => () => background.close(), [background])
  useEffect(() => { if (online) { void background.load(); void learning.load() } }, [background, learning, online, host.backgroundEpoch])
  const mode = learning.value?.mode
  return <section className="memoria-cartao" aria-labelledby="memoria-cartao-titulo">
    <h3 id="memoria-cartao-titulo" className="memoria-titulo">{t('memoria.pagina')}</h3>
    <p className="memoria-nota">{t('memoria.cartao.lead')}</p>
    <div className="memoria-colunas">
      <ModelColumn t={t} state={state} model={background} online={online} off={mode === 'off'} />
      <div className="memoria-coluna">
        <h4>{t('memoria.aprende.titulo')}</h4>
        {learning.value
          ? <LearningChoice t={t} selection={learning.value.mode} disabled={!online || learning.busy} onChange={(next) => void learning.save(next)} />
          : learning.issue === 'unavailable' && <p role="status" className="memoria-aviso">{t('memoria.aprende.indisponivel')}</p>}
        {learning.issue === 'save' && <p role="alert" className="memoria-aviso">{t('memoria.aprende.erro')}</p>}
        <p className="memoria-nota memoria-fixa">{t('memoria.aprende.nota')}</p>
        {!!learning.value?.pending && <p role="status" className="memoria-contador">{t('memoria.sugestoes.esperando', { n: learning.value.pending })}</p>}
      </div>
    </div>
  </section>
}

function ModelColumn({ t, state, model, online, off }: { t: Tradutor; state: BackgroundState; model: BackgroundStore; online: boolean; off: boolean }) {
  const idioma = useIdioma()
  const data = state.data
  const draft = state.draft
  const row = data?.functions.find((f) => f.definition.id === 'memory_consolidation')
  const disabled = !online || state.busy || state.loading || !data?.document || !!data.accountsIssue
  const accounts = data?.accounts ?? []
  const account = accounts.find((a) => a.id === draft?.accountId)
  const models = account?.models ?? []
  const label = (id: string) => {
    const family = modelFamily(id)
    return family ? t(FAMILY[family]) : id
  }
  const modelId = draft?.mode === 'custom' ? draft.modelId : ''
  const notices = row ? modelNotices(row.resolution, state) : null
  const pick = (id: string) => { for (const patch of modelPatch(id)) model.change(patch) }
  return <div className="memoria-coluna">
    <h4>{t('memoria.modelo.titulo')}</h4>
    {state.error && !data ? <p role="status" className="memoria-aviso">{t('memoria.modelo.erro')}</p> : draft && row && <>
      <label className="memoria-campo"><span>{t('memoria.modelo.motor')}</span>
        <ChoicePicker language={superficieIdioma(idioma)} selection={draft.accountId} disabled={disabled}
          options={[{ id: '', label: t('memoria.modelo.escolhaMotor') }, ...accountOptions(accounts, savedAccountId(row.selection), accountWords(t), 'memory_consolidation')]}
          onChange={(accountId) => model.change({ accountId })} /></label>
      <label className="memoria-campo"><span>{t('memoria.modelo.modelo')}</span>
        <ChoicePicker language={superficieIdioma(idioma)} selection={modelId} disabled={disabled || !account}
          options={[{ id: '', label: t('memoria.modelo.automatico') }, ...models.map((m) => ({ id: m.id, label: label(m.id) })),
            ...(modelId && !models.some((m) => m.id === modelId) ? [{ id: modelId, label: t('memoria.modelo.indisponivelNaConta') }] : [])]}
          onChange={pick} /></label>
      {notices?.saved && <p role="status" className="memoria-nota">{t(notices.saved)}</p>}
      {notices?.warning && <p role="status" className="memoria-aviso">{t(notices.warning)}</p>}
      {state.dirty && <div className="memoria-acoes">
        <Button kind="secondary" disabled={state.busy} onClick={() => void model.load(true)}>{t('memoria.modelo.descartar')}</Button>
        <Button disabled={disabled || state.conflict || !data || !validDraft(data, draft)} onClick={() => void model.save()}>{t('memoria.modelo.salvar')}</Button>
      </div>}
      {state.conflict && <div className="memoria-acoes" role="alert"><span className="memoria-aviso">{t('memoria.modelo.conflito')}</span>
        <Button kind="secondary" disabled={state.busy} onClick={() => void model.load(true)}>{t('memoria.modelo.recarregar')}</Button></div>}
    </>}
    <p className="memoria-nota">{t(off ? 'memoria.frequencia.desligada' : 'memoria.frequencia')}</p>
  </div>
}

const LEARNING_TEXT = {
  auto: ['memoria.aprende.auto', 'memoria.aprende.auto.detalhe'],
  suggest: ['memoria.aprende.sugerir', 'memoria.aprende.sugerir.detalhe'],
  off: ['memoria.aprende.nada', 'memoria.aprende.nada.detalhe'],
} as const

/** O mesmo rádio das Permissões do criar bot (linha realçada, anel e bolinha em accent), com o teclado do ChoiceList. */
function LearningChoice({ t, selection, disabled, onChange }: { t: Tradutor; selection: Learning; disabled: boolean; onChange: (id: Learning) => void }) {
  const rows = useRef<(HTMLButtonElement | null)[]>([])
  const stop = radioTabStop(LEARNINGS, selection)
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = radioArrowTarget(e, index, LEARNINGS.length)
    if (next === null) return
    e.preventDefault()
    rows.current[next]?.focus()
    if (LEARNINGS[next] !== selection) onChange(LEARNINGS[next]!)
  }
  return <div role="radiogroup" aria-label={t('memoria.aprende.titulo')} className="memoria-modos">
    {LEARNINGS.map((id, i) => <button key={id} ref={(el) => { rows.current[i] = el }} type="button" role="radio" aria-checked={id === selection}
      tabIndex={i === stop ? 0 : -1} disabled={disabled} className="memoria-modo" onClick={() => { if (id !== selection) onChange(id) }} onKeyDown={(e) => onKeyDown(e, i)}>
      <span className="memoria-radio" aria-hidden="true" />
      <span className="memoria-modo-texto"><strong>{t(LEARNING_TEXT[id][0])}</strong><small>{t(LEARNING_TEXT[id][1])}</small></span>
    </button>)}
  </div>
}

/** Quantos bots leem "Sobre você": só os do espaço Pessoal com a memória compartilhada ligada (host/src/chat/memory/mod.rs). */
function leitores(host: BotStore) {
  const pessoal = host.spaces?.personalSpaceId
  return [...host.bots.values()].filter((b) => b.kind !== 'group' && b.sharedMemory && (!b.spaceId || !pessoal || b.spaceId === pessoal)).length
}

/** MX-8: "Sobre você", os cinco campos da nota global. Sem computador conectado, nada é lido nem gravado. */
function SobreVoceCard({ host: source }: { host: BotStore }) {
  const t = useT()
  const host = useModel(source)
  const online = host.connectedClient !== null
  // Só a identidade durável (computador, Hub, dono) recria o store; conexão e época não: cair e voltar mantém o rascunho.
  const chave = identidade(host)
  // O rascunho é do store: fica no pai, para a página voltar com o que foi digitado (estado-pagina.ts).
  const [store0] = useEstadoDaPagina(`memoria/sobre-voce/${chave}`, () => new SobreVoceStore(async () => {
    const connected = source.connectedClient
    if (!connected) throw new Error('memory_connection_unavailable')
    return connected
  }))
  const store = useModel(store0)
  useEffect(() => { if (online) void store.load() }, [store, online])
  // `secret_detected`: o texto fica e o foco vai ao campo, para o dono tirar o trecho e salvar de novo.
  const campos = useRef<Partial<Record<Campo, HTMLTextAreaElement | null>>>({})
  const alvo = store.campoDoSegredo
  useEffect(() => { if (alvo) campos.current[alvo]?.focus() }, [alvo])
  const bloqueado = !online || store.ocupado || store.salvo === null
  const n = leitores(host)
  return <section className="memoria-cartao sv-cartao" aria-labelledby="sv-titulo">
    <h3 id="sv-titulo" className="memoria-titulo">{t('memoria.sobreVoce.titulo')}</h3>
    <p className="memoria-nota">{t('memoria.sobreVoce.lead')}</p>
    {online && <p className="memoria-nota sv-regra"><Icon name="person.2" size={12} color="var(--context-ink-2, var(--ink-2))" /><span>{t.plural('memoria.sobreVoce.leitores', n)}</span></p>}
    {store.problema === 'indisponivel' && <p role="status" className="memoria-aviso">{t('memoria.sobreVoce.indisponivel')}</p>}
    {store.problema === 'leitura' && <div role="alert" className="sv-problema"><span className="memoria-aviso">{t('memoria.sobreVoce.erro')}</span>
      <Button kind="secondary" disabled={!online || store.ocupado} onClick={() => void store.load()}>{t('memoria.sobreVoce.tentarDeNovo')}</Button></div>}
    <div className="sv-campos">
      {CAMPOS.map((c: Campo) => <label key={c} className={`sv-campo sv-${c}`}>
        <span className="sv-rotulo">{t(`memoria.sobreVoce.${c}`)}</span>
        <textarea ref={(el) => { campos.current[c] = el }} dir="auto" rows={2} disabled={bloqueado} aria-invalid={alvo === c || undefined} placeholder={t(`memoria.sobreVoce.${c}.dica`)} value={store.rascunho[c]} onChange={(e) => store.editar(c, e.target.value)} aria-describedby={[alvo === c ? 'sv-segredo' : '', c === 'evitar' ? 'sv-evitar-nota' : ''].filter(Boolean).join(' ') || undefined} />
        {/* Decisão do dono (11/11): proibição vai para Nunca fazer (MX-111); sem link até a página existir. */}
        {c === 'evitar' && <small id="sv-evitar-nota" className="memoria-nota sv-nota">{t('memoria.sobreVoce.evitar.nota')}</small>}
      </label>)}
    </div>
    <div className="sv-rodape">
      <span className="memoria-nota sv-status" role="status">
        {store.gravou && !store.sujo ? t('memoria.sobreVoce.salvo') : store.soltas > 0 ? t.plural('memoria.sobreVoce.soltas', store.soltas) : null}
      </span>
      {store.problema === 'conflito' && <span role="alert" className="memoria-aviso">{t('memoria.sobreVoce.conflito')}</span>}
      {store.problema === 'gravacao' && <span role="alert" className="memoria-aviso">{t('memoria.sobreVoce.erroGravar')}</span>}
      {store.problema === 'segredo' && <span id="sv-segredo" role="alert" className="memoria-aviso sv-segredo">{t('memoria.sobreVoce.segredo')}</span>}
      {store.problema === 'conflito'
        ? <Button kind="secondary" disabled={!online || store.ocupado} onClick={() => store.descartar()}>{t('memoria.sobreVoce.recarregar')}</Button>
        : store.sujo && <>
          <Button kind="secondary" disabled={store.ocupado} onClick={() => store.descartar()}>{t('memoria.sobreVoce.descartar')}</Button>
          <Button disabled={bloqueado} onClick={() => void store.save()}>{t(store.problema === 'gravacao' ? 'memoria.sobreVoce.tentarDeNovo' : 'memoria.sobreVoce.salvar')}</Button>
        </>}
    </div>
  </section>
}

/** MX-8: um aviso só, no alto da página, valendo para os dois cartões. */
function SemConexao({ host: source }: { host: BotStore }) {
  const t = useT()
  const host = useModel(source)
  return host.connectedClient ? null : <p role="status" className="memoria-aviso memoria-sem-conexao">{t('memoria.semConexao')}</p>
}
