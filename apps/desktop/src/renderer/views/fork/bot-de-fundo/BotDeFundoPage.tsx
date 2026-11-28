import { Fragment, useEffect, useMemo, useSyncExternalStore } from 'react'
import { Button, CardForm, CardSection, ChoicePicker, IconButton, Spinner, ValueRow } from '../../../components/Controls'
import { Spacer } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import { Icon } from '../../../components/Icon'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { capacidadeSpace } from '../../../store/fork-space-contrato'
import { PersonalTile, SpaceTile } from '../spaces/SpaceTile'
import { EscolhaDaFuncao } from './EscolhaDaFuncao'
import { fraseDaFuncao, rascunhoDaEscolha, resumoDaEscolha, seletorDaFuncao, type SemEscolha, type TextosSeletor } from './seletor'
import { provedoresDoHub } from '../seletor-modelo/catalogo'
import { settingsScreen } from '../ajustes/tela'
import { textosEm } from '../contas-ia/textos'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { BackgroundStore, accountOptions, savedAccountId, bindingOf, canBind, functionName, selecaoDoSpace, spaceDaPagina, computadorDaPagina, grupoDe, KNOWN_FUNCTIONS, MEMORIA_OCIOSA_MIN, automaticBackend, reasonText, shownReason, accountDisconnected, isDisconnected, suggestedAccount, taskKey, spaceChoiceIssue, spaceChoiceText, validDraft, type Account, type AccountWords, type BackgroundState, type Draft, type FunctionRow, type FunctionSnapshot, type Grupo } from './model'
import { isolado } from '../../../../shared/bidi.ts'
import { BotaoAbrirContas } from '../contas-ia/FalhaDeConta'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import './bot-de-fundo.css'

/** Os motivos que se resolvem em Contas de IA (conectar, reconectar, confirmar): o botão leva até lá. */
const MOTIVOS_DE_CONTA = new Set(['account_missing', 'account_choice_required', 'account_disconnected', 'billing_unknown', 'api_not_enabled', 'subscription_unverified', 'accounts_unavailable'])

/** Estados em frase simples (o leigo não sabe o que é "Não confirmada"). */
const ESTADOS = { connected: 'fundo.conta.estado.connected', unverified: 'fundo.conta.estado.unverified', unavailable: 'fundo.conta.estado.unavailable', disconnected: 'fundo.conta.estado.disconnected' } as const
/** O motivo em linguagem simples; conta desconectada diz "desconectada" e o que a função deixa de fazer (mesmos termos da Atividade, MEX-1355). */
function motivoTexto(t: Tradutor, reason: string | undefined, row: Pick<FunctionRow, 'definition' | 'selection'>, accounts: Account[]) {
  const shown = shownReason(reason, row, accounts)
  return shown === 'account_disconnected' ? t('fundo.motivo.desconectada', { tarefa: t(taskKey(row.definition.id)) }) : t(reasonText(shown, row))
}
function scopeOf(host: BotStore) { return `${host.hello?.hostId ?? ''}/${host.principalEpoch}/${host.connectedClient ? 'connected' : 'unavailable'}` }
/** Nome e estado com os textos de Contas de IA; a recusa do Hub vem do catálogo de idiomas. */
const accountWords = (t: Tradutor, idioma: AppLanguage): AccountWords => ({ legacy: textosEm(idioma).perfilAntigo, subscription: textosEm(idioma).assinatura, refused: t('memoria.modelo.conta.naoFuncionaBotDeFundo'), state: (state) => t(Object.hasOwn(ESTADOS, state) ? ESTADOS[state as keyof typeof ESTADOS] : 'fundo.conta.estado.disconnected') })
const accountChoices = (accounts: Account[], selected: string, saved: string, functionId: string, words: AccountWords, t: Tradutor) => [{ id: '', label: t('fundo.escolhaConta') }, ...accountOptions(accounts, saved, words, functionId), ...(selected && !accounts.some((a) => a.id === selected) ? [{ id: selected, label: t('fundo.contaIndisponivel') }] : [])]
function choiceText(data: FunctionSnapshot, row: FunctionRow, t: Tradutor) {
  if (row.definition.managedInVoice) return t('fundo.gerenciadoVoz')
  const binding = bindingOf(row.selection)
  if (!binding) return row.resolution.status === 'unavailable' ? motivoTexto(t, row.resolution.unavailable.reason, row, data.accounts) : t('fundo.modeloNaoInformado')
  return `${data.accounts.find((a) => a.id === binding.accountId)?.label ?? t('fundo.contaIndisponivelCurta')} · ${binding.modelId}`
}
/** O nome da função: do catálogo para as conhecidas; outra, o nome que o Hub mandou. */
const nomeFuncao = (row: FunctionRow, t: Tradutor) => { const n = functionName(row); return 'key' in n ? t(n.key) : n.label }

export function BotDeFundoPage({ openVoice, openActivity }: { openVoice?: () => void; openActivity: () => void }) {
  const app = useApp()
  const [escolhido, setComputer] = useEstadoDaPagina('bot-de-fundo/computador', () => app.selectedStore?.computer.id ?? (app.computers.length === 1 ? app.computers[0]!.id : ''))
  // Um computador por vez (decisão do dono D1): com Space ativo é o dele; só com o Pessoal aberto a pessoa escolhe.
  const ativo = app.spaceMirrors.active
  const computer = computadorDaPagina(ativo, escolhido)
  const host = computer ? app.store(computer) : null
  const spaceId = host ? spaceDaPagina(ativo, computer, host.hello?.hostId, host.spaces?.personalSpaceId) : null
  const t = useT()
  const idioma = useIdioma()
  return <div className="background-page idioma-superficie" {...superficieIdioma(idioma)}>
    <CardForm>
      {/* Entrega 4 (MX-7): primeiro o que é; o computador só aparece quando há mais de um. */}
      <div className="background-sobre"><p>{t('fundo.intro')}</p><button type="button" className="ajustes-link" onClick={openActivity}>{t('fundo.verAtividade')}</button></div>
      {!ativo && app.computers.length > 1 && <CardSection><ValueRow label={t('fundo.computador')}><ChoicePicker selection={computer} options={[{ id: '', label: t('fundo.escolhaComputador') }, ...app.computers.map((c) => ({ id: c.id, label: c.name }))]} onChange={setComputer} language={superficieIdioma(idioma)} /></ValueRow></CardSection>}
      {host ? <ComputerBackground key={`${computer}/${ativo?.spaceId ?? ''}`} host={host} openVoice={openVoice} openActivity={openActivity} spaceId={spaceId} /> : <Text tone="secondary">{t('fundo.escolhaComputadorTexto')}</Text>}
    </CardForm>
  </div>
}

const GRUPOS = [
  { id: 'emUso', titulo: 'fundo.grupo.emUso', ajuda: 'fundo.grupo.emUso.ajuda' },
  { id: 'emPreparo', titulo: 'fundo.grupo.emPreparo', ajuda: 'fundo.grupo.emPreparo.ajuda' },
  { id: 'funcoes', titulo: 'fundo.grupo.funcoes', ajuda: 'fundo.grupo.funcoes.ajuda' },
  { id: 'voz', titulo: 'fundo.grupo.voz', ajuda: 'fundo.grupo.voz.ajuda' },
] as const satisfies readonly { id: Grupo; titulo: string; ajuda: string }[]
const proposito = (row: FunctionRow): `fundo.proposito.${typeof KNOWN_FUNCTIONS[number]}` | null => (KNOWN_FUNCTIONS as readonly string[]).includes(row.definition.id) ? `fundo.proposito.${row.definition.id as typeof KNOWN_FUNCTIONS[number]}` : null

function ComputerBackground({ host: source, openVoice, openActivity, spaceId }: { host: BotStore; openVoice?: () => void; openActivity: () => void; spaceId: string | null }) {
  const host = useModel(source)
  const t = useT()
  const idioma = useIdioma()
  const scope = scopeOf(host)
  const client = host.connectedClient
  const model = useMemo(() => new BackgroundStore(async () => {
    const connected = host.connectedClient
    if (!connected) throw new Error('background_connection_unavailable')
    return connected
  }, () => scopeOf(host), () => host.connectedClient), [host, scope, client])
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot)
  const online = client !== null
  // Seleções da página ficam no pai (estado-pagina.ts): a página volta como estava.
  // Por Hub e Space: um item aberto de um Hub nunca abre no outro (estado-pagina.ts).
  const [aberta, setAberta] = useEstadoDaPagina<string | null>(`bot-de-fundo/${host.computer.id}/${spaceId ?? ''}/aberta`, null)
  // Decisão do dono (11/11): Em preparo começa recolhido, atrás de "Ver N em preparo".
  const [verPreparo, setVerPreparo] = useEstadoDaPagina(`bot-de-fundo/${host.computer.id}/${spaceId ?? ''}/preparo`, false)
  useEffect(() => () => model.close(), [model])
  useEffect(() => { if (online) void model.load() }, [model, online, host.backgroundEpoch])
  const data = state.data
  const abrir = (id: string) => {
    if (aberta === id) { if (!state.dirty) setAberta(null); return }
    model.select(id)
    if (model.getSnapshot().selectedId === id) setAberta(id)
  }
  // Revisão R1 (MX-7): Recarregar fica num cabeçalho que não depende dos dados, na página; uma leitura que
  // falhou (sem dados) ainda tem por onde tentar de novo.
  const cabecalho = <div className="card-section-title background-cabecalho">
    <Text as="h3" size="body" weight="semibold" tone="text">{t('fundo.cabecalho.funcoes')}</Text>
    <Spacer />
    {online && (state.loading ? <Spinner size={12} /> : <IconButton title={state.dirty ? t('fundo.recarregarDescartar') : t('fundo.recarregar')} icon="arrow.clockwise" disabled={state.busy} onClick={() => void model.load(true)} />)}
  </div>
  // Space (não o Pessoal) num Hub sem o contrato de fundo: a leitura falha por falta de recurso, não por erro.
  const spaceSemSuporte = !!spaceId && spaceId !== host.spaces?.personalSpaceId && capacidadeSpace(host.hello, 'spaceBackgroundContract') < 1
  const erro = state.error === 'leitura' && spaceSemSuporte ? 'semRecurso' : state.error
  const status = <>
    {!online && <div className="card-section-rows"><p role="status" className="background-vazio"><Icon name="exclamationmark.circle" size={18} />{t('fundo.semConexao')}</p></div>}
    {state.aviso && <p role="status" className="background-status">{t(`fundo.aviso.${state.aviso}`)}</p>}
    {erro && <div className="card-section-rows">{erro === 'semRecurso'
      // Falta de recurso do Hub não é falha: estado informativo calmo, sem alerta (como em Lugares de trabalho).
      ? <p role="status" className="background-vazio"><Icon name="info.circle" size={18} />{t('fundo.erro.semRecurso')}</p>
      : <p role="alert" className="background-vazio"><Icon name="exclamationmark.circle" size={18} />{t(`fundo.erro.${erro}`)}</p>}</div>}
  </>
  const linha = (row: FunctionRow) => data && <div key={row.definition.id} className="background-row-wrap">
            <button type="button" className="background-row" aria-expanded={aberta === row.definition.id} disabled={state.busy} onClick={() => abrir(row.definition.id)}>
              <div className="background-row-text"><strong>{nomeFuncao(row, t)}</strong>{proposito(row) && <span>{t(proposito(row)!)}</span>}</div>
              <span className="background-row-choice">{choiceText(data, row, t)}</span>
              <Icon name="chevron.right" size={10} weight="semibold" className="background-row-chevron" />
            </button>
            {aberta === row.definition.id && state.selectedId === row.definition.id && (state.draft ? <FunctionEditor host={host} row={row} data={data} draft={state.draft} model={model} online={online} openActivity={openActivity} /> : <p className="background-help">{t('fundo.selecaoIndisponivel')}</p>)}
          </div>
  return <>
    <section className="card-section">{cabecalho}{status}</section>
    <Espacos host={host} model={model} state={state} online={online} spaceId={spaceId} />
    {data && GRUPOS.map((g) => {
      const rows = data.functions.filter((row) => grupoDe(row) === g.id)
      if (!rows.length) return null
      if (g.id === 'emPreparo') {
        // Rascunho aberto numa função em preparo: não recolhe por cima dele.
        const preso = verPreparo && state.dirty && rows.some((row) => row.definition.id === aberta)
        const alternar = <button type="button" className="background-row background-ver" aria-expanded={verPreparo} aria-controls="background-em-preparo" disabled={preso} onClick={() => setVerPreparo(!verPreparo)}>
          <div className="background-row-text"><strong>{verPreparo ? t('fundo.grupo.emPreparo.ocultar') : t('fundo.grupo.emPreparo.ver', { n: formatarNumero(idioma, rows.length) })}</strong></div>
          <Icon name="chevron.right" size={10} weight="semibold" className="background-row-chevron" />
        </button>
        return <section key={g.id} className="card-section" id="background-em-preparo-secao">
          <div className="card-section-title"><Text as="h3" size="body" weight="semibold" tone="text">{t(g.titulo)}</Text><span className="background-badge">{t('fundo.grupo.emPreparo.marca')}</span></div>
          <div className="card-section-rows" id="background-em-preparo">
            <div className="card-row">{alternar}</div>
            {verPreparo && rows.map((row) => <Fragment key={row.definition.id}><div className="hairline inset" /><div className="card-row">{linha(row)}</div></Fragment>)}
          </div>
          {verPreparo && <div className="card-section-footer">{t(g.ajuda)}</div>}
        </section>
      }
      return <CardSection key={g.id} title={t(g.titulo)} footer={t(g.ajuda)}>
        {rows.map((row) => g.id === 'voz'
          ? <div key={row.definition.id} className="background-row"><div className="background-row-text"><strong>{nomeFuncao(row, t)}</strong>{proposito(row) && <span>{t(proposito(row)!)}</span>}</div></div>
          : linha(row))}
        {g.id === 'voz' && openVoice ? <div className="background-row"><Spacer /><Button kind="secondary" onClick={openVoice}>{t('fundo.abrirVoz')}</Button></div> : null}
      </CardSection>
    })}
    {!data && online && state.loading && <p role="status" className="background-status"><Spinner size={12} /> {t('fundo.carregando')}</p>}
  </>
}

/** "Neste Space": só a seleção do Space ativo; Hub e contrato iguais, as dos outros Spaces seguem salvas. */
function Espacos({ host, model, state, online, spaceId }: { host: BotStore; model: BackgroundStore; state: BackgroundState; online: boolean; spaceId: string | null }) {
  const t = useT()
  const data = state.data
  const row = state.spaces ? selecaoDoSpace(state.spaces.selections, spaceId) : null
  const space = row ? host.spaces?.spaces.find((s) => s.id === row.spaceId && s.revision === row.spaceRevision) : undefined
  const escolha = row ? spaceChoiceText(data, row) : null, issue = row ? spaceChoiceIssue(data, row) : null
  return <CardSection title={t('fundo.espacos.titulo')} footer={t('fundo.espacos.ajuda')}>
    <div className="background-info"><p>{t('fundo.espacos.texto')}</p></div>
    {state.spacesError && <p role="status" className="background-info">{t('fundo.espacos.nenhumaPresumida', { erro: t('fundo.espaco.indisponivel') })}</p>}
    {row && escolha && <div className="background-space" key={row.spaceId}>
      {space?.kind === 'personal' ? <PersonalTile owner={space.owner} /> : space?.tile ? <SpaceTile tile={space.tile} /> : null}
      <div><h2><bdi>{row.spaceName}</bdi></h2><span className="background-badge">{row.state === 'configured' ? t('fundo.espaco.configurado') : row.state === 'stale' ? t('fundo.espaco.desatualizado') : t('fundo.espaco.semAutorizacao')}</span><p>{t('fundo.espaco.escolha', { escolha: 'key' in escolha ? t(escolha.key) : isolado(escolha.label) })}</p>{issue && <p role="status" className="background-help">{t(issue)}</p>}<p className="background-help">{row.state === 'stale' ? t('fundo.espaco.mudou') : t('fundo.espaco.aguarda')}</p></div>
      <div className="background-space-actions">{row.state !== 'configured' && <Button kind="secondary" disabled={!online || state.busy || state.loading || state.dirty || !canBind(data, row)} onClick={() => void model.saveSpace(row, true)}>{t('fundo.espaco.usarSelecao')}</Button>}{row.state !== 'not_authorized' && <Button kind="secondary" disabled={!online || state.busy || state.loading} onClick={() => void model.saveSpace(row, false)}>{t('fundo.espaco.remover')}</Button>}</div>
    </div>}
    {state.spaces && !row && <p className="background-help">{t('fundo.espaco.nenhum')}</p>}
    {!state.spaces && !state.loading && !state.spacesError && <p className="background-help">{t('fundo.espaco.indisponivel')}</p>}
  </CardSection>
}

/** Por que a escolha salva não vem marcada, na frase da tela (cada chave com os seus dados). */
function textoSemEscolha(t: Tradutor, s: SemEscolha): string {
  switch (s.chave) {
    case 'fundo.seletor.salvaConta': return t(s.chave, s.params)
    case 'fundo.seletor.salvaModelo': return t(s.chave, s.params)
    case 'fundo.seletor.salvaOpcao': return t(s.chave, s.params)
    case 'fundo.seletor.automaticoFora': return t(s.chave, s.params)
  }
}

/** Os textos do seletor da função (desenho v2), no idioma da tela. */
function textosDoSeletor(t: Tradutor, idioma: AppLanguage): TextosSeletor {
  return {
    legacy: textosEm(idioma).perfilAntigo,
    estado: { desconectada: t('fundo.seletor.estado.desconectada'), indisponivel: t('fundo.seletor.estado.indisponivel'), naoConfirmada: t('fundo.seletor.estado.naoConfirmada'), motorRecusado: '', apiEmBreve: t('fundo.seletor.estado.apiEmBreve'), semAssinatura: t('fundo.seletor.estado.semAssinatura'), semModelo: t('fundo.seletor.estado.semModelo'), conectada: t('fundo.seletor.estado.conectada') },
    conectar: t('fundo.seletor.acao.conectar'), confirmarLogin: t('fundo.seletor.acao.confirmarLogin'),
    automatico: { nome: t('fundo.seletor.automatico'), nomeCodex: t('fundo.seletor.automaticoCodex'), etiqueta: t('fundo.seletor.recomendado'), meta: t('fundo.seletor.hojeHaiku') },
    nota: t('fundo.seletor.nota'),
    recusadoCurto: t('fundo.seletor.motorRecusadoCurto'), recusado: (motor) => t('fundo.seletor.motorRecusado', { motor }),
    semConta: (motor) => t('fundo.seletor.semConta', { motor }), trocarDica: (conta) => t('fundo.seletor.trocarDica', { conta }), automaticoFora: (conta) => t('fundo.seletor.automaticoFora', { conta }),
  }
}

function FunctionEditor({ host, row, data, draft, model, online, openActivity }: { host: BotStore; row: FunctionRow; data: FunctionSnapshot; draft: Draft; model: BackgroundStore; online: boolean; openActivity: () => void }) {
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot)
  const t = useT()
  const idioma = useIdioma()
  const language = superficieIdioma(idioma)
  const words = accountWords(t, idioma)
  const textos = textosDoSeletor(t, idioma)
  const accounts = draft.mode === 'automatic' ? data.accounts.filter((a) => a.backend === automaticBackend(draft.functionId)) : data.accounts
  // Conta desconectada nunca vem escolhida: o campo abre vazio e a tela avisa, com a conta pronta que o Hub aceitaria.
  const desconectada = accountDisconnected(data.accounts, draft.accountId, row)
  const sugerida = desconectada ? suggestedAccount(accounts, draft.functionId, draft.mode === 'automatic') : null
  // O botão usa o rótulo do conjunto completo do seletor, que traz o identificador das contas de mesmo nome.
  const rotuloDe = (lista: Account[], conta: Account) => accountOptions(lista, '', words, draft.functionId).find((o) => o.id === conta.id)?.label ?? ''
  // A alternativa (Haiku) também nunca aparece escolhida desconectada; não mexe na conta principal.
  const contasHaiku = data.accounts.filter((a) => a.backend === 'claude')
  const altDesconectada = draft.mode === 'automatic' && draft.functionId === 'bot_draft' && isDisconnected(data.accounts, draft.alternativeAccountId)
  const altId = altDesconectada ? '' : draft.alternativeAccountId
  const altSugerida = altDesconectada ? suggestedAccount(contasHaiku, draft.functionId) : null
  const resolution = state.loading ? null : state.check ?? row.resolution
  const disabled = !online || state.busy || state.loading || !data.document || !!data.accountsIssue
  // A frase do que acontece acompanha a escolha mostrada no campo (nada de frase para conta que não serve).
  const seletor = seletorDaFuncao({ data, functionId: draft.functionId, draft, catalogo: provedoresDoHub(host), desconectada, textos, abrirContas: () => undefined })
  const resumo = resumoDaEscolha(seletor, textos)
  const frase = fraseDaFuncao(draft.functionId, resumo, draft.mode === 'automatic', t(seletor.valor.backend === 'claude' ? 'fundo.seletor.modeloLeve' : 'fundo.seletor.modeloPadraoCodex'), MEMORIA_OCIOSA_MIN)
  const nenhumaPronta = seletor.contas.length === 0
  return <section className="background-editor" aria-label={nomeFuncao(row, t)}>
    {frase && <p className="background-frase">{frase.chave === 'fundo.frase.memoria' ? t(frase.chave, frase.params) : t(frase.chave, frase.params)} <button type="button" className="ajustes-link" onClick={openActivity}>{t('fundo.seletor.verEmAtividade')}</button></p>}
    {nenhumaPronta && !data.accountsIssue && <div className="background-help" role="status"><p>{t('fundo.seletor.nenhumaPronta', { tarefa: t(taskKey(draft.functionId)) })}</p><div className="background-actions"><Button onClick={() => settingsScreen.show('ai-accounts', 'contas')}>{t('fundo.seletor.conectarConta')}</Button></div></div>}
    {desconectada && !state.dirty && <div className="background-help" role="alert">
      <p>{t('fundo.motivo.desconectada', { tarefa: t(taskKey(draft.functionId)) })}</p>
      <div className="background-actions"><BotaoAbrirContas label={t('fundo.seletor.reconectar')} />{sugerida && <Button kind="secondary" disabled={disabled} onClick={() => model.change({ accountId: sugerida.id })}>{t('fundo.editor.usarConta', { conta: rotuloDe(accounts, sugerida) })}</Button>}</div>
    </div>}
    <label className="background-campo"><span>{t('fundo.seletor.rotulo')}</span>
      <EscolhaDaFuncao host={host} data={data} functionId={draft.functionId} draft={draft} desconectada={desconectada} dirty={state.dirty} disabled={disabled} textos={textos} onEscolher={(escolha) => model.choose(rascunhoDaEscolha(escolha))} />
    </label>
    {seletor.semEscolha && <p className="background-help" role="status">{textoSemEscolha(t, seletor.semEscolha)}</p>}
    {altDesconectada && <div className="background-help" role="alert">
      <p>{t('fundo.editor.alternativaDesconectada')}</p>
      <div className="background-actions"><BotaoAbrirContas />{altSugerida && <Button kind="secondary" disabled={disabled} onClick={() => model.change({ alternativeAccountId: altSugerida.id })}>{t('fundo.editor.usarConta', { conta: rotuloDe(contasHaiku, altSugerida) })}</Button>}</div>
    </div>}
    {draft.mode === 'automatic' && draft.functionId === 'bot_draft' && <label><span>{t('fundo.editor.contaHaiku')}</span><ChoicePicker selection={altId} options={accountChoices(contasHaiku, altId, altDesconectada ? '' : savedAccountId(row.selection, true), draft.functionId, words, t)} disabled={disabled} onChange={(alternativeAccountId) => model.change({ alternativeAccountId })} language={language} /></label>}
    {/* Entrega 4 (MX-7): o estado em uma frase simples; sem o botão Conferir (a releitura já confere). */}
    {!(desconectada && !state.dirty) && <p className={`background-result${!state.dirty && resolution?.status === 'available' ? ' ok' : ''}`} role="status">{!state.dirty && resolution?.status === 'available' && <Icon name="checkmark.circle" size={13} />}{state.dirty ? t('fundo.editor.confirmaAoSalvar') : !resolution ? t('fundo.editor.aguarde') : resolution.status === 'available' ? t('fundo.editor.pronto') : motivoTexto(t, resolution.unavailable.reason, row, data.accounts)}{resolution?.status === 'unavailable' && !desconectada && MOTIVOS_DE_CONTA.has(resolution.unavailable.reason ?? '') && <BotaoAbrirContas />}</p>}

    {grupoDe(row) === 'emPreparo' && <p className="background-help">{t('fundo.editor.emPreparo')}</p>}
    {state.conflict && <p role="alert">{t('fundo.editor.conflito')}</p>}
    <div className="background-actions">{state.dirty && <Button kind="secondary" disabled={state.busy} onClick={() => void model.load(true)}>{t('fundo.editor.descartar')}</Button>}<Button disabled={disabled || !state.dirty || state.conflict || !validDraft(data, draft)} onClick={() => void model.save()}>{state.dirty && resumo ? t('fundo.seletor.usar', { conta: resumo.conta }) : t('fundo.editor.salvar')}</Button></div>
  </section>
}
