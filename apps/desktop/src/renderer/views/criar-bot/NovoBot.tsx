import { confirmarConta, contaConfirmadaPara, reconciliarConfirmacao, type ConfirmacaoConta } from '@shared/fork-criar-bot-confirmacao'
import { ContaConfirmadaTexto, BotCriadoNaOrigemTexto } from './ContaConfirmada'
import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { chooseNewBotPermission, inheritNewBotPermission } from '@shared/bot-autonomy'
import { avatarDoBot, avatarHubNotice, avatarMexeriquinhaHubNotice, camposAvatarDoBot, normalizarAvatarDoBot, randomAvatar, roleSuggestion } from '@shared/fork-avatar-config'
import { erroDaFuncao, limparFuncao, type Funcao } from '@shared/fork-bot-funcao'
import { rotuloDoTitulo } from './rotulos'
import { FUNCOES_DA_CRIACAO, PASSOS, funcoesDaCriacao, anterior, nomesDoPapel, papeisComuns, pedirAutonomia, proximo, sortearNome, type Passo } from '@shared/fork-criar-bot'
import { contaPadraoDoHub, contasDoMotor, corpoDaCriacao, escolherMotor, falhaDosPadroes, type FalhaDosPadroes, erroDaCriacao, escolhaDeConta, motorComConta, noSpace, spacesDaCriacao, type Destino } from '@shared/fork-criar-bot-destino'
import { SpaceBotCreation } from '../../store/fork-space-bot-creation'
import { aplicarEscolha, valorDoSeletor } from '@shared/fork-criar-bot-modelo'
import type { ManagedAccount } from '@shared/fork-engine-accounts'
import { normalizeBot } from '@shared/models'
import { Button, ChoicePicker, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader, useDismiss } from '../../components/Overlay'
import { useBotMood } from '../../lib/donnabot-avatars/src'
import { MODELOS } from '../../lib/mexeriquinhas/index.ts'
import { camposDaCriacao, comAvatar, configDoPronto } from '../fork/mexeriquinha/seletor'
import { useStore } from '../../store/context'
import { engineSetup } from '../../store/fork-engine-setup'
import type { CreationAvailability } from '../../lib/creation-target'
import { fillDefaults, newDraft } from '../bots/drafts'
import { ContaDaIa, EscolhaDaEquipe, OndeTrabalha } from './Destino'
import { destinoTravado, gravarEquipe, opcoesDeEquipe, tentarDeNovo, type EscolhaEquipe, type PosCriacao } from '../fork/criar-bot-equipe/equipe'
import { TimesSession } from '../fork/times/times-session'
import { timesScreen } from '../fork/times/navigation'
import { useModel } from '../../lib/observable'
import type { ComputerChoice } from '../bots/NewBotView'
import { FuncaoPainel, type Aba, type Escolha } from './FuncaoPainel'
import { ModeloDaIa } from './ModeloDaIa'
import { PassoFerramentas, PassoPapel, PassoPermissoes } from './Passos'
import { Previa } from './Previa'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import '../bots/bots.css'
import './criar-bot.css'
import '../fork/idioma/escritas.css'

// Janela estreita ou baixa: a prévia vira uma faixa no topo (proposta DON-865, 760×500).
const COMPACTA = '(max-width: 900px), (max-height: 620px)'
const assinarLargura = (aviso: () => void) => {
  const consulta = window.matchMedia(COMPACTA)
  consulta.addEventListener('change', aviso)
  return () => consulta.removeEventListener('change', aviso)
}
const PAPEIS = papeisComuns()

/** Falha ao preparar os padrões: código estável; a tela escolhe o texto. */
type FalhaPadroes = FalhaDosPadroes

/**
 * Novo bot em quatro passos (DON-865, D365): prévia ao vivo à esquerda, "Configure seu bot" à direita.
 * Envia ao Hub só o que a folha já mandava (nome, função, avatar, motor, modelo e autonomia); conta de
 * IA, Space, equipe, pastas e ferramentas ficam para as próximas fatias. "Criar já" cria em qualquer passo.
 */
export function NovoBotView({ computer, availability, onAbrirSpace, spaceCreation }: {
  computer: ComputerChoice | null
  availability?: CreationAvailability
  /** Fatia 2: depois de criar num Space, abre esse Space (o store desta folha só espelha o Pessoal). */
  onAbrirSpace?: (spaceId: string, botId: string) => void
  spaceCreation?: SpaceBotCreation
}) {
  const t = useT()
  const idioma = useIdioma()
  const store = useStore()
  const [creationOrigin] = useState(() => ({ hostId: store.hello?.hostId, client: store.connectedClient, epoch: store.principalEpoch, hubName: store.computer.name }))
  const dismiss = useDismiss()
  const [draft, setDraft] = useState(newDraft)
  const [backendChosen, setBackendChosen] = useState(false)
  const [defaultsReady, setDefaultsReady] = useState(false)
  const [defaultsError, setDefaultsError] = useState<FalhaPadroes | null>(null)
  const [escolha, setEscolha] = useState<Escolha>(null)
  const [passo, setPasso] = useState<Passo>('papel')
  const [estudio, setEstudio] = useState<Aba | null>(null)
  const [confirmarLiberar, setConfirmarLiberar] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [destino, setDestino] = useState<Destino>({ conta: null, spaceId: spaceCreation?.spaceId ?? null })
  const [confirmacaoConta, setConfirmacaoConta] = useState<ConfirmacaoConta | null>(null)
  const [criadoNaOrigem, setCriadoNaOrigem] = useState<{ nome: string; space: string; hub: string } | null>(null)
  // Fatia 3: equipe e chefe. Uma sessão de Equipes só desta folha (nunca a da tela Equipes, que pode ter rascunho).
  const [times] = useState(() => new TimesSession())
  useModel(times)
  const [equipe, setEquipe] = useState<EscolhaEquipe | null>(null)
  const [pos, setPos] = useState<PosCriacao | null>(null)
  const travado = destinoTravado(pos)
  const [contas, setContas] = useState<{ estado: 'carregando' | 'pronto' | 'falhou'; lista: ManagedAccount[] }>({ estado: 'carregando', lista: [] })
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  const sugerido = useRef('')
  const compacta = useSyncExternalStore(assinarLargura, () => window.matchMedia(COMPACTA).matches)
  const { mood, play } = useBotMood('idle')
  // Criar exige só o Hub com avatar (como antes); sem a v2, o seletor fica desabilitado e a criação manda o formato antigo.
  const avatarNotice = avatarHubNotice(store.hello)
  const avisoV2 = avatarNotice ? null : avatarMexeriquinhaHubNotice(store.hello)

  useEffect(() => {
    let live = true
    setDefaultsReady(false)
    void Promise.all([store.teamSettings(), store.refreshBackends()]).then(([settings, refreshed]) => {
      if (!live) return
      if (!refreshed) {
        setDefaultsError('motores')
        return
      }
      setDraft((d) => inheritNewBotPermission(d, settings.permission))
      setDefaultsError(null)
      setDefaultsReady(true)
    }).catch((e: unknown) => { if (live) setDefaultsError(falhaDosPadroes(e)) })
    return () => { live = false }
  }, [store, store.connection.kind])

  // Fatia 2: as contas do Hub, só com a capacidade engine.accounts respondida pelo Hub (SAAS-04). Só leitura.
  const podeConta = store.canManageEngineAccounts
  useEffect(() => {
    if (!podeConta || store.connection.kind !== 'online') return
    let live = true
    setContas((c) => ({ ...c, estado: 'carregando' }))
    void store.ready().then((client) => engineSetup.accounts(client)).then(
      (r) => { if (live) setContas({ estado: 'pronto', lista: r.accounts }) },
      () => { if (live) setContas({ estado: 'falhou', lista: [] }) },
    )
    return () => { live = false }
  }, [store, podeConta, store.connection.kind])

  const personalId = store.spaces?.personalSpaceId ?? null
  const emSpace = noSpace(destino, personalId)
  const sugeridoMotor = fillDefaults(store, draft).backend
  const backend = backendChosen ? draft.backend : emSpace && !motorComConta(sugeridoMotor) ? 'codex' : sugeridoMotor
  const escolhaConta = escolhaDeConta(podeConta, backend)
  const contasDoBackend = emSpace ? contas.lista.filter((c) => c.backend === backend).map((conta) => ({ conta, escolhivel: conta.state === 'connected' })) : contasDoMotor(contas.lista, backend)
  const contaPadrao = emSpace ? contaPadraoDoHub(contas.lista, backend) : null
  const contaEscolhida = escolhaConta === 'escolher'
    ? destino.conta ? contasDoBackend.find((c) => c.conta.id === destino.conta && c.escolhivel)?.conta ?? null : contaPadrao
    : null
  const destinoEnviado: Destino = { conta: contaEscolhida?.id ?? null, spaceId: destino.spaceId }
  const contextoConta = { hubId: store.hello?.hostId, epoch: store.principalEpoch, backend, accountId: contaEscolhida?.id ?? null, spaceId: destino.spaceId }
  const contaConfirmada = contaConfirmadaPara(confirmacaoConta, contextoConta, destinoEnviado.conta)
  useEffect(() => { setConfirmacaoConta((previous) => reconciliarConfirmacao(previous, contextoConta)) },
    [backend, store.hello?.hostId, store.principalEpoch, contaEscolhida?.id, destino.spaceId])
  const contaInvalida = emSpace ? !contaEscolhida || contas.estado !== 'pronto' : !!destino.conta && !contaEscolhida
  const spaces = spacesDaCriacao(store.spaces)
  const lugarDaEquipe = destino.spaceId && destino.spaceId !== personalId ? destino.spaceId : null
  useEffect(() => {
    setEquipe(null)
    times.open(store, lugarDaEquipe)
    return () => times.close()
  }, [times, store, lugarDaEquipe])
  const opcoesEquipe = times.phase.kind === 'ready' && times.snapshot ? opcoesDeEquipe(times.snapshot) : []
  const estadoEquipe = times.phase.kind === 'ready' ? { kind: 'pronto' as const }
    : times.phase.kind === 'error' ? { kind: 'erro' as const, falha: times.phase.failure } : { kind: 'carregando' as const }
  const equipeEscolhida = equipe ? opcoesEquipe.find((o) => o.id === equipe.areaId) ?? null : null
  const chefeEscolhido = equipeEscolhida?.chefes.find((c) => c.id === equipe?.chefeId)?.nome ?? t('botCreate.voce')
  const resumoEquipe = equipeEscolhida ? t('botCreate.resumoEquipeCom', { equipe: equipeEscolhida.nome, chefe: chefeEscolhido }) : t('botCreate.semEquipe')
  const onde = (destino.spaceId && spaces.find((x) => x.id === destino.spaceId && x.id !== personalId)?.name) || t('botCreate.ondePessoal')
  const motor = store.hello?.backends.find((b) => b.id === backend)?.name ?? backend
  // Fase 2a: a aparência é sempre uma Mexeriquinha (v2); as funções de exemplo trazem a aparência antiga e viram v2 aqui.
  const config = normalizarAvatarDoBot(avatarDoBot(draft))
  const funcao = draft.title ?? ''
  const funcaoErro = erroDaFuncao(funcao)
  // A tela mostra o rótulo traduzido; o Hub recebe o `title` original (o valor do exemplo ou o digitado).
  const funcaoLimpa = funcaoErro ? '' : rotuloDoTitulo(t, limparFuncao(funcao))
  const ready = !pos && !criadoNaOrigem && defaultsReady && !avatarNotice && !funcaoErro && !saving && !availability?.error && store.connection.kind === 'online'
    && !contaInvalida && (!emSpace || contaConfirmada)
  const proprias = funcoesDaCriacao(emSpace ? [] : [...store.bots.values()], store.principal.id ?? store.principal.protectedId)
  const nome = draft.name.trim()

  const create = async () => {
    availability?.check()
    spaceCreation?.check()
    if (contaInvalida || (emSpace && !contaConfirmada)) return
    if (emSpace && (store.hello?.hostId !== creationOrigin.hostId || store.connectedClient !== creationOrigin.client
      || store.principalEpoch !== creationOrigin.epoch)) throw new Error('stale_space_context')
    if (!defaultsReady) return
    // Send exactly the engine shown, including an explicit choice that needs setup.
    const { mexeriquinha: _v2, ...base } = fillDefaults(store, draft)
    const corpo = corpoDaCriacao({ ...base, name: nome, backend, title: limparFuncao(funcao), ...camposDaCriacao(draft, store.hello) }, destinoEnviado, personalId)
    if (emSpace && !contaConfirmadaPara(confirmacaoConta, contextoConta, corpo.forkEditor?.engineAccountId ?? null)) throw new Error('account_unavailable')
    if (corpo.spaceId && noSpace(destinoEnviado, personalId)) {
      // O store desta folha espelha só o Pessoal: o bot do Space não entra nele; a tela abre o Space.
      const target = spaceCreation ?? new SpaceBotCreation(store, corpo.spaceId)
      const bot = await target.create(corpo)
      return { id: bot.id, nome: bot.name || nome, spaceId: corpo.spaceId, creation: target, hub: creationOrigin.hubName, space: onde }
    }
    const bot = await store.save(corpo)
    return { id: bot.id, nome: bot.name || nome, spaceId: null, creation: null, hub: creationOrigin.hubName, space: onde }
  }
  /** Fim da criação: abre o Space, ou seleciona o bot no Pessoal, e fecha a folha. */
  const terminar = (criado: { id: string; spaceId: string | null }) => {
    if (criado.spaceId) onAbrirSpace?.(criado.spaceId, criado.id)
    else store.setSelection(criado.id)
    dismiss()
  }
  /** Passo 2 (fatia 3): o quadro de Equipes. Falhou, o bot continua criado; Tentar de novo só refaz este passo. */
  const colocarNaEquipe = async (criado: { id: string; nome: string; spaceId: string | null }, escolha: EscolhaEquipe, nomeEquipe: string) => {
    const pedido: PosCriacao = { ...criado, equipe: { escolha, nome: nomeEquipe }, fase: 'gravando' }
    setPos(pedido)
    const r = await gravarEquipe(times, criado.id, escolha)
    if (r.ok) return terminar(criado)
    setPos({ ...pedido, fase: 'falhou', codigo: r.codigo })
  }
  /** Tentar de novo: sempre o pedido guardado (equipe, chefe e lugar originais), nunca o que estiver na tela. */
  const novaTentativa = async (anterior: PosCriacao) => {
    setPos({ ...anterior, fase: 'gravando', codigo: undefined })
    const r = await tentarDeNovo(times, anterior, lugarDaEquipe)
    if (r.ok) return terminar(anterior)
    setPos({ ...anterior, fase: 'falhou', codigo: r.codigo })
  }
  const submit = () => {
    if (!ready) return
    setSaving(true)
    setError(null)
    const escolha = equipe && equipeEscolhida ? { escolha: equipe, nome: equipeEscolhida.nome } : null
    create()
      .then((criado) => {
        if (!criado || (criado.creation && !alive.current)) return
        if (criado.creation && !criado.creation.isCurrent()) { setCriadoNaOrigem({ nome: criado.nome, space: criado.space, hub: criado.hub }); return }
        if (!escolha) return terminar(criado)
        return colocarNaEquipe(criado, escolha.escolha, escolha.nome)
      })
      .catch((e: unknown) => setError(t(`botCreate.erroCriacao.${erroDaCriacao(e)}`)))
      .finally(() => setSaving(false))
  }

  const escolher = (f: Funcao) => {
    const antes = sugerido.current
    sugerido.current = f.nomeDoBot
    setEscolha(f.id)
    setDraft((d) => ({ ...comAvatar(d, f.config), name: roleSuggestion(d.name, antes, f.nomeDoBot), title: f.valor }))
    play('happy')
  }
  const nova = () => {
    const antes = sugerido.current
    sugerido.current = ''
    setEscolha('nova')
    setDraft((d) => ({ ...d, name: roleSuggestion(d.name, antes, ''), title: '' }))
  }
  const sortear = () => {
    const papel = [...FUNCOES_DA_CRIACAO, ...proprias].find((f) => f.id === escolha)
    const novo = sortearNome(nomesDoPapel(papel), draft.name)
    sugerido.current = novo
    setDraft((d) => ({ ...d, name: novo }))
    play('blink')
  }
  const pedir = (permissao: 'ask' | 'auto') => {
    const { aplicar, confirmar } = pedirAutonomia(draft.permission, permissao)
    if (confirmar) setConfirmarLiberar(true)
    if (aplicar) setDraft((d) => chooseNewBotPermission(d, aplicar))
  }

  const previa = normalizeBot({ id: 'novo-bot-previa', name: nome || t('botCreate.semNome'), title: funcaoLimpa,
    ...camposAvatarDoBot(config), backend, managedWorkspace: true, status: 'idle' })
  const indice = PASSOS.indexOf(passo)
  const seguinte = proximo(passo)
  const voltar = anterior(passo)

  if (criadoNaOrigem) return <div className="nb idioma-superficie" {...superficieIdioma(idioma)}>
    <p role="status"><BotCriadoNaOrigemTexto t={t} {...criadoNaOrigem} /></p>
    <Button onClick={dismiss}>{t('botCreate.fechar')}</Button>
  </div>

  return <div className={`nb idioma-superficie ${compacta ? 'nb-compacta' : ''}`} {...superficieIdioma(idioma)}>
    <ModalHeader closeTitle={t('botCreate.fechar')} title={t('botCreate.titulo')} trailing={<span className="nb-pontos" role="img" aria-label={t('botCreate.progresso', { atual: indice + 1, total: PASSOS.length })}>
      {PASSOS.map((p, i) => <span key={p} className={`nb-pontinho ${i === indice ? 'nb-atual' : i < indice ? 'nb-feito' : ''}`} />)}
    </span>} />
    {emSpace ? <div className="nb-conta-confirmacao">
      <label><input type="checkbox" checked={contaConfirmada && !contaInvalida} disabled={saving || !!pos || contaInvalida}
        onChange={(e) => setConfirmacaoConta(e.target.checked ? confirmarConta(contextoConta) : null)} />
        <ContaConfirmadaTexto t={t} conta={contaEscolhida?.label ?? t('botCreate.contaPadraoDono')} hub={creationOrigin.hubName} motor={motor} /></label>
      <button type="button" disabled={saving || !!pos} onClick={() => setPasso('motor')}>{t('botCreate.escolherContaSpace')}</button>
      {contaInvalida && contas.estado !== 'carregando' ? <p role="alert">{t('botCreate.contaSpaceIndisponivel')}</p> : null}
    </div> : null}
    <div className="nb-corpo">
      <Previa store={store} compacta={compacta} config={config} mood={mood} nome={draft.name} funcao={funcaoLimpa} motor={motor} onde={onde} equipe={resumoEquipe}
        liberar={draft.permission === 'auto'} bot={previa} onNome={(name) => setDraft((d) => ({ ...d, name }))} onSortear={sortear}
        onAleatorio={() => { setDraft((d) => comAvatar(d, avisoV2 ? randomAvatar() : configDoPronto(MODELOS[Math.floor(Math.random() * MODELOS.length)]!, config.estilo))); play('surprise') }}
        onPersonalizar={() => setEstudio('aparencia')} onEnter={submit} />

      <section className="nb-passos">
        {estudio ? <div className="nb-estudio">
          <button type="button" className="nb-voltar-passos" onClick={() => setEstudio(null)}><Icon name="chevron.left" size={12} />{t('botCreate.voltarPassos')}</button>
          {avatarNotice
            ? <p className={`nb-nota ${store.hello ? 'nb-erro' : ''}`} role="status">{avatarNotice}</p>
            : <FuncaoPainel key={estudio} inicial={estudio} config={config} avisoAparencia={avisoV2} escolha={escolha} funcao={rotuloDoTitulo(t, funcao)} erro={funcaoErro} proprias={proprias}
              onConfig={(avatar) => setDraft((d) => comAvatar(d, avatar))} onEscolher={escolher} onNova={nova}
              onFuncao={(title) => {
                setEscolha((e) => e ?? 'nova')
                setDraft((d) => ({ ...d, title }))
              }} />}
        </div> : <>
          <div className="nb-passos-topo">
            <h3 className="nb-titulo">{t('botCreate.configure')}</h3>
            <p className="nb-nota"><Icon name="pencil" size={12} /> {t('botCreate.editavel')}</p>
            <div role="tablist" aria-label={t('botCreate.passos')} className="nb-trilha">
              {PASSOS.map((p, i) => <button key={p} type="button" role="tab" aria-selected={p === passo} className="nb-trilha-passo" onClick={() => setPasso(p)}>
                <span className={`nb-trilha-num ${i < indice ? 'nb-feito' : ''}`} aria-hidden="true">{i < indice ? <Icon name="checkmark" size={10} /> : i + 1}</span>
                {t(`botCreate.passo.${p}`)}
              </button>)}
            </div>
          </div>
          <div role="tabpanel" aria-label={t(`botCreate.passo.${passo}`)} className="nb-passo-corpo" key={passo}>
            {avatarNotice && passo === 'papel' ? <p className={`nb-nota ${store.hello ? 'nb-erro' : ''}`} role="status">{avatarNotice}</p> : null}
            {passo === 'papel' ? <PassoPapel papeis={PAPEIS} escolha={escolha} onEscolher={escolher} onVerTodas={() => setEstudio('role')}
              equipe={<EscolhaDaEquipe estado={estadoEquipe} opcoes={opcoesEquipe} escolha={equipe} travado={travado} onEscolha={(e) => { if (!travado) setEquipe(e) }} />}
              onde={<OndeTrabalha spaces={spaces} personalId={personalId} contrato={store.hello?.spacesContract === 1 && !!store.spaces}
                spaceId={destino.spaceId} travado={travado || !!spaceCreation} onSpace={(spaceId) => { if (!travado && !spaceCreation) { setDestino((d) => ({ ...d, spaceId })); setConfirmacaoConta(null) } }} />} /> : null}
            {passo === 'motor' ? <ModeloDaIa backends={(store.hello?.backends ?? []).filter((b) => !emSpace || motorComConta(b.id))} valor={valorDoSeletor(draft, backend, contaEscolhida?.id ?? null)}
              disabled={!defaultsReady} conta={contaEscolhida}
              contaSlot={<ContaDaIa escolha={escolhaConta} contas={contasDoBackend} carregando={contas.estado === 'carregando'} falhou={contas.estado === 'falhou'}
                conta={destino.conta} padraoDisponivel={!emSpace || !!contaPadrao} disabled={!defaultsReady} onConta={(conta) => {
                  // Outra conta: o modelo volta ao padrão (a lista de modelos é a da conta).
                  setDestino((d) => ({ ...d, conta }))
                  setConfirmacaoConta(null)
                  setDraft((d) => ({ ...d, model: null, modelOptions: {} }))
                }} />}
              onEscolha={(escolha) => {
                // Fatia 4: o seletor só devolve a escolha; ela vai ao rascunho e o createBot grava.
                const r = aplicarEscolha(draft, destino, backend, escolha)
                if (r.motorTrocado || r.destino.conta !== destino.conta) setConfirmacaoConta(null)
                if (r.motorTrocado) setBackendChosen(true)
                setDestino(r.destino)
                setDraft(r.draft)
              }}
              onBackend={(id) => {
                // Motor já ativo: nada muda. Trocar de verdade limpa conta, modelo e opções juntos.
                const troca = escolherMotor(draft, destino, backend, id)
                if (!troca) return
                setConfirmacaoConta(null)
                setBackendChosen(true)
                setDestino(troca.destino)
                // O modo rápido é do motor: o novo começa no padrão (velocidade normal).
                setDraft({ ...troca.draft, fastMode: false })
              }} /> : null}
            {passo === 'permissoes' ? <PassoPermissoes permissao={draft.permission} disabled={!defaultsReady} onPedir={pedir} /> : null}
            {passo === 'ferramentas' ? <PassoFerramentas /> : null}
            {defaultsError ? <p className="nb-nota nb-erro" role="alert">{t(`botCreate.erro.${defaultsError}`)}</p> : null}
          </div>
        </>}

        {pos ? <div className="nb-rodape nb-pos" role={pos.fase === 'falhou' ? 'alert' : 'status'}>
          {pos.fase === 'gravando'
            ? <p className="nb-nota"><Spinner size={12} /> {t('botCreate.posGravando', { nome: pos.nome, equipe: pos.equipe.nome })}</p>
            : <div className="nb-pos-texto">
              <p className="nb-pos-titulo"><Icon name="exclamationmark.triangle" size={13} /> {t('botCreate.posFalhou', { nome: pos.nome, equipe: pos.equipe.nome })}</p>
              <p className="nb-nota">{t(`botCreate.falhaEquipe.${pos.codigo ?? 'recusada'}`)}</p>
            </div>}
          <span className="nb-espaco" />
          {pos.fase === 'falhou' ? <>
            <Button kind="secondary" onClick={() => terminar(pos)}>{t('botCreate.fechar')}</Button>
            <Button kind="secondary" onClick={() => {
              if (!pos.spaceId) store.setSelection(pos.id)
              timesScreen.show(pos.spaceId, pos.spaceId ? store.computer.id : null)
              dismiss()
            }}>{t('botCreate.abrirEquipes')}</Button>
            <Button onClick={() => void novaTentativa(pos)}>{t('botCreate.tentarEquipe')}</Button>
          </> : null}
        </div> : <div className="nb-rodape">
          {error || availability?.error ? <p className="nb-nota nb-erro fade-in" role="alert">{error || availability?.error}</p> : null}
          {voltar ? <Button kind="secondary" onClick={() => setPasso(voltar)}>{t('botCreate.voltar')}</Button>
            : <Button kind="secondary" onClick={dismiss}>{t('botCreate.cancelar')}</Button>}
          {seguinte ? <button type="button" className="nb-criar-ja" disabled={!ready} onClick={submit}>{t('botCreate.criarJa')}</button> : null}
          <span className="nb-espaco" />
          {computer ? <label className="nb-computador">
            <span>{t('botCreate.computador')}</span>
            <ChoicePicker language={superficieIdioma(idioma)} selection={computer.selection} options={computer.options} onChange={computer.onChange} />
          </label> : null}
          {seguinte ? <Button onClick={() => { setEstudio(null); setPasso(seguinte) }}>{t('botCreate.continuar')}</Button>
            : <>
              {compacta ? null : <span className="nb-nota">{t('botCreate.acordarLinha')}</span>}
              <Button className="nb-acordar" onClick={submit} disabled={!ready}>{saving ? <Spinner /> : nome ? t('botCreate.acordar', { nome }) : t('botCreate.acordarSemNome')}</Button>
            </>}
        </div>}
      </section>
    </div>

    <Dialog language={superficieIdioma(idioma)} open={confirmarLiberar} title={t('botCreate.liberarTitulo', { nome: nome || t('botCreate.semNome') })} message={t('botCreate.liberarMensagem', { nome: nome || t('botCreate.semNome') })}
      cancel={t('botCreate.manterPedir')} onClose={() => setConfirmarLiberar(false)}
      actions={[{ title: t('botCreate.liberarMesmoAssim'), destructive: true, action: () => setDraft((d) => chooseNewBotPermission(d, 'auto')) }]} />
  </div>
}
