import { useEffect, useState } from 'react'
import { carregarModelos, provedoresDoHub, useCatalogoMudou } from '../seletor-modelo/catalogo'
import type { AccountUsage, Engine, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import type { Bot } from '../../../../shared/models.ts'
import { isolado, ltr } from '../../../../shared/bidi.ts'
import { Button, CardSection, IconButton, ProgressBar, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog, ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { BotAvatar } from '../../../fork/avatars/Avatar'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { engineSetup } from '../../../store/fork-engine-setup'
import { AgentTile } from '../../marketplace/AgentSheet'
import { EmptyState, Field, FormSheet, MarketBadge } from '../../marketplace/pieces'
import { AdicionarConexao } from './AdicionarConexao'
import { ChaveDeApi } from './ChaveDeApi'
import { ConectarConta } from './ConectarConta'
import { contasFor, type ContasStore } from './contas-store'
import { NovaConta } from './NovaConta'
import { SeEstaContaAcabar } from './SeEstaContaAcabar'
import { accountKind, accountView, viaProgramaOficial, botNames, engineName, engineRegistry, errorMessage, rowActions, testResult, validLabel, whenText, type AccountAction } from './regras'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { formatarNumero, traduzir } from '../../../lib/fork-i18n/index.ts'
import './contas-ia.css'

/** A lista de contas do Hub desta view, a mesma para a página e para o Motor do bot. */
export function useContas(store: BotStore): ContasStore {
  return useModel(contasFor(store, () => store.ready(), engineSetup))
}

type Route =
  | { kind: 'pick' }
  | { kind: 'new'; engine?: Engine }
  | { kind: 'connect'; id: string; reconnect: boolean }
  | { kind: 'key'; id: string }
  | { kind: 'details'; id: string }

/** Contas de IA de um Hub: listar, adicionar, conectar, reconectar, renomear, desconectar e remover. */
export function ContasDoHub({ forBot, onChanged }: {
  /** Aberto pelo Motor de um bot: depois de conectar, oferece usar a conta nele. */
  forBot?: { name: string; engine?: Engine; use: (accountId: string) => void }
  onChanged?: () => void
}) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const open = window.codync.appScheme === 'mexerica-open'
  const store = useStore()
  const contas = useContas(store)
  const [route, setRoute] = useState<Route | null>(null)
  const online = store.connection.kind === 'online' && !store.mismatch
  // O Hub decide a capacidade; a rota de rede não concede autoridade.
  const local = store.canManageEngineAccounts
  const accounts = contas.accounts ?? []

  useEffect(() => {
    if (online) void contas.load()
  }, [contas, online])
  useEffect(() => { if (!local) setRoute(null) }, [local])

  const close = () => {
    setRoute(null)
    onChanged?.()
  }
  const run = (account: ManagedAccount, action: AccountAction | 'new') => {
    if (action === 'refresh') void contas.load(local)
    // Perfil antigo desconectado: o Hub não o reconecta; entra uma conta nova do mesmo motor.
    else if (action === 'new') setRoute({ kind: 'new', engine: account.backend })
    else if (action === 'key' && store.isLoopback) setRoute({ kind: 'key', id: account.id })
    else setRoute({ kind: 'connect', id: account.id, reconnect: action === 'reconnect' })
  }
  const routed = route && route.kind !== 'new' && route.kind !== 'pick' ? contas.account(route.id) : null

  return (
    // A lista inteira declara o idioma (lang/dir) sem mudar o layout de quem a contém.
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection
        title={textos.hub}
        accessory={online ? (
          contas.loading ? <Spinner size={12} /> : <IconButton title={textos.atualizar} icon="arrow.clockwise" onClick={() => void contas.load(local)} />
        ) : null}
        footer={textos.rodape}
      >
        <HStack gap={8}>
          <Icon name="server.rack" size={13} color="var(--context-ink-2, var(--ink-2))" />
          <Text size="caption" tone="secondary" grow>{local ? (store.isLoopback ? textos.ligadoPorEsteMac : textos.donoPelaNuvem) : textos.remotoIndisponivel}</Text>
          {online && local ? <Button onClick={() => setRoute(forBot?.engine ? { kind: 'new', engine: forBot.engine } : { kind: 'pick' })}>{textos.adicionar}</Button> : null}
        </HStack>
        {!online ? <Text tone="secondary">{textos.hubDesconectado}</Text> : null}
        {online && contas.error ? <Text tone="danger" role="alert" className="selectable">{errorMessage(contas.error, idioma)}</Text> : null}
        {online && contas.accounts === null && contas.loading ? <Text tone="secondary" role="status">{textos.carregando}</Text> : null}
        {online && contas.accounts !== null && accounts.length === 0 ? (
          <EmptyState icon="key" title={textos.vazioTitulo} detail={open ? traduzir(idioma, 'contas.vazioOpen') : textos.vazioDetalhe} />
        ) : null}
        {online ? accounts.map((account) => (
          <ContaLinha key={account.id} store={store} contas={contas} account={account} local={local && (accountKind(account, contas.kindOf(account.id)) !== 'api' || store.isLoopback)} onAction={run} onOpen={() => setRoute({ kind: 'details', id: account.id })} onChanged={onChanged} />
        )) : null}
      </CardSection>

      <FormSheet language={superficieIdioma(idioma)} open={route !== null && (route.kind === 'new' || route.kind === 'pick' || routed !== null)} onClose={close} width={560} height={580} fixed>
        {route?.kind === 'pick' ? (
          <AdicionarConexao contas={contas} onPlano={(engine) => setRoute({ kind: 'new', engine })} onClose={close} />
        ) : route?.kind === 'new' ? (
          <NovaConta
            contas={contas}
            initialEngine={route.engine}
            onCreated={(account, kind) => setRoute(kind === 'api' ? { kind: 'key', id: account.id } : { kind: 'connect', id: account.id, reconnect: false })}
            onClose={close}
          />
        ) : route?.kind === 'connect' && routed ? (
          <ConectarConta key={routed.id} contas={contas} account={routed} reconnect={route.reconnect} forBot={forBot && routed.backend === (forBot.engine ?? routed.backend) ? forBot : undefined}
            onKey={() => { contas.learnKind(routed.id, 'api'); setRoute({ kind: 'key', id: routed.id }) }} onClose={close} />
        ) : route?.kind === 'key' && routed ? (
          <ChaveDeApi key={routed.id} contas={contas} account={routed} onClose={close} />
        ) : route?.kind === 'details' && routed ? (
          <ContaDetalhe key={routed.id} store={store} contas={contas} account={routed} local={local} onAction={(action) => run(routed, action)} onClose={close} />
        ) : null}
      </FormSheet>
    </div>
  )
}

/** O que o Testar da linha mostra: rodando, o resultado do Hub para o estado de agora, ou o erro do pedido. */
type Teste = 'rodando' | { state: ManagedAccount['state'] } | { error: unknown }

function ContaLinha({ store, contas, account, local, onAction, onOpen, onChanged }: {
  store: BotStore
  contas: ContasStore
  account: ManagedAccount
  local: boolean
  onAction: (account: ManagedAccount, action: AccountAction | 'new') => void
  onOpen: () => void
  onChanged?: () => void
}) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const kind = accountKind(account, contas.kindOf(account.id))
  const view = accountView(account, kind, idioma)
  const acoes = rowActions(account, kind, idioma)
  const bots = botNames(account.botIds, (id) => store.bots.get(id)?.name ?? null, idioma)
  const pending = contas.operationFor(account.id) !== null
  const [teste, setTeste] = useState<Teste | null>(null)
  const [confirmar, setConfirmar] = useState<'excluir' | 'em-uso' | null>(null)
  const [erro, setErro] = useState<unknown>(null)
  const testando = teste === 'rodando'
  // O resultado só vale para o estado em que foi dado: mudou (reconectou, atualizou), some e nunca volta.
  useEffect(() => { setTeste((atual) => (atual === 'rodando' || (atual && 'state' in atual && atual.state === account.state) ? atual : null)) }, [account.state, account.revision])
  const ocupado = contas.busy !== null
  // Entrega 3: os bots viram rostos à direita; a linha de baixo diz o motor, a cobrança e quantos modelos o seletor mostra.
  const rostos = account.botIds.map((id) => store.bots.get(id)).filter((b): b is Bot => b !== undefined)
  const ocultos = (account.hiddenModelIds ?? []).filter((id) => account.models.some((m) => m.id === id)).length
  const subtitle = [engineName(account.backend), view.billing, ocultos ? textos.modelosResumo(account.models.length - ocultos, account.models.length) : '', bots ? '' : textos.nenhumBot].filter(Boolean).join(' · ')
  const nome = isolado(view.title)
  const testar = async () => {
    setErro(null)
    setTeste('rodando')
    try {
      const fresh = await contas.verify(account)
      setTeste({ state: fresh.state })
    } catch (e) {
      setTeste({ error: e })
    }
  }
  // Reativar é um pedido ao Hub; falha aparece na própria linha, sem rota nova.
  const reativar = async () => {
    setErro(null)
    try {
      await contas.setEnabled(account, true)
      onChanged?.()
    } catch (e) {
      setErro(e)
    }
  }
  const excluir = async () => {
    setErro(null)
    setTeste(null)
    try {
      await contas.remove(account)
      onChanged?.()
    } catch (e) {
      setErro(e)
    }
  }
  const resultado = teste === null || teste === 'rodando' ? null
    : 'error' in teste ? { ok: false, text: textos.testeFalhou(errorMessage(teste.error, idioma)) }
    : teste.state === account.state ? testResult(account, idioma) : null
  return (
    <HStack gap={12} className="contas-linha">
      <button className="contas-linha-abrir" onClick={onOpen} aria-label={`${isolado(view.title)}, ${view.state}. ${textos.detalhes}`}>
        <AgentTile registry={engineRegistry(account.backend)} icon={18} tile={32} radius={9} />
        <VStack as="span" gap={3} grow shrink>
          <HStack as="span" gap={6} shrink>
            <Text tone="text" weight="medium" truncate dir="auto" title={isolado(view.title)}>{view.title}</Text>
            <MarketBadge label={pending ? textos.conectando : view.state} tone={pending ? 'secondary' : view.tone} icon={pending ? undefined : view.tone === 'added' ? 'checkmark' : view.attention ? 'exclamationmark' : undefined} />
            {viaProgramaOficial(account, kind) ? <MarketBadge label={textos.programaOficial} title={textos.programaOficialDetalhe} /> : null}
          </HStack>
          <Text size="caption" tone="secondary" truncate title={subtitle}>{subtitle}</Text>
        </VStack>
      </button>
      {bots ? (
        <span className="contas-rostos" role="img" aria-label={textos.usadoPor(bots)} title={textos.usadoPor(bots)}>
          {rostos.slice(0, 4).map((bot) => <span key={bot.id} className="contas-rosto"><BotAvatar bot={bot} size={22} animated={false} /></span>)}
          {account.botIds.length > Math.min(rostos.length, 4) ? <span className="contas-rosto contas-rosto-mais">+{formatarNumero(idioma, account.botIds.length - Math.min(rostos.length, 4))}</span> : null}
        </span>
      ) : null}
      {local && pending ? (
        <button className="capsule-action" onClick={() => onAction(account, account.state === 'disconnected' ? 'connect' : 'reconnect')}>{textos.continuar}</button>
      ) : local ? (
        <>
          {acoes.test && window.codync.appScheme !== 'mexerica-open' ? (
            <button className="capsule-action" disabled={ocupado} aria-label={`${textos.testar}: ${nome}`} onClick={() => void testar()}>
              {testando ? <Spinner size={11} /> : textos.testar}
            </button>
          ) : null}
          {acoes.connect ? (
            <button className="capsule-action" disabled={ocupado} aria-label={`${acoes.connect.label}: ${nome}`} onClick={() => { setTeste(null); setErro(null); if (acoes.connect!.action === 'enable') void reativar(); else onAction(account, acoes.connect!.action) }}>{acoes.connect.label}</button>
          ) : view.primary?.action === 'refresh' && !acoes.test ? (
            <button className="capsule-action" disabled={ocupado} onClick={() => onAction(account, view.primary!.action)}>{view.primary.label}</button>
          ) : null}
          {acoes.remove ? (
            <button className="capsule-action contas-excluir" disabled={ocupado} aria-label={`${textos.excluir}: ${nome}`} onClick={() => setConfirmar(acoes.remove === 'in-use' ? 'em-uso' : 'excluir')}>{textos.excluir}</button>
          ) : null}
        </>
      ) : null}
      <Icon name="chevron.right" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />
      {testando ? <Text size="caption" tone="secondary" role="status" className="contas-linha-resultado">{textos.testando}</Text> : null}
      {resultado ? <Text size="caption" tone={resultado.ok ? 'secondary' : 'danger'} role={resultado.ok ? 'status' : 'alert'} className="contas-linha-resultado selectable">{resultado.ok ? <Icon name="checkmark" size={10} /> : null} {resultado.text}</Text> : null}
      {erro ? <Text size="caption" tone="danger" role="alert" className="contas-linha-resultado selectable">{errorMessage(erro, idioma)}</Text> : null}
      <Dialog
        open={confirmar === 'excluir'}
        title={textos.excluirTitulo}
        message={account.origin === 'legacy' ? textos.excluirMensagemAntigo(view.title) : textos.excluirMensagemLivre(view.title)}
        cancel={textos.cancelar}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmar(null)}
        actions={[{ title: textos.excluir, destructive: true, action: () => void excluir() }]}
      />
      <Dialog
        open={confirmar === 'em-uso'}
        title={textos.excluirEmUsoTitulo}
        message={textos.excluirEmUsoMensagem(bots)}
        cancel={textos.fechar}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmar(null)}
        actions={[]}
      />
    </HStack>
  )
}

/** Tudo de uma conta: estado e prova, cobrança, bots que a usam, uso e as ações de gestão. */
export function ContaDetalhe({ store, contas, account, local, onAction, onClose }: {
  store: BotStore
  contas: ContasStore
  account: ManagedAccount
  local: boolean
  onAction: (action: AccountAction) => void
  onClose: () => void
}) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const kind = accountKind(account, contas.kindOf(account.id))
  const view = accountView(account, kind, idioma)
  const [usage, setUsage] = useState<AccountUsage | null | 'erro'>(null)
  const [renaming, setRenaming] = useState(false)
  const [label, setLabel] = useState(account.label)
  const [confirm, setConfirm] = useState<'disconnect' | 'disable' | 'remove' | null>(null)
  // Entrega 3: ocultar um modelo que algum bot desta conta usa pede confirmação, com os nomes.
  const [ocultarEmUso, setOcultarEmUso] = useState<{ id: string; nome: string; bots: string } | null>(null)
  // DON-917: o erro cru do Hub, ou `comBots` (a frase sai na hora, com os nomes de agora).
  const [error, setError] = useState<unknown>(null)
  const [comBots, setComBots] = useState(false)
  const managed = account.origin === 'managed'
  const bots = account.botIds.map((id) => store.bots.get(id)).filter((b): b is Bot => b !== undefined)
  const names = botNames(account.botIds, (id) => store.bots.get(id)?.name ?? null, idioma)
  const busy = contas.busy !== null
  // Entrega 3: os nomes dos modelos vêm do catálogo do motor (o mesmo do seletor); sem ele, vale o id.
  useCatalogoMudou()
  useEffect(() => { if (window.codync.appScheme !== 'mexerica-open') carregarModelos(store, account.backend) }, [store, account.backend])
  const catalogo = provedoresDoHub(store).find((p) => p.id === account.backend)?.models ?? []
  const nomeModelo = (id: string | null | undefined) => (id ? (catalogo.find((m) => m.id === id)?.name ?? id) : textos.botModeloPadrao)
  const hidden = account.hiddenModelIds ?? []
  const visiveis = account.models.filter((m) => !hidden.includes(m.id)).length
  // A Open não verifica a conta à parte, não lê uso nem reserva e não oculta modelos: o Hub dela recusa essas chamadas.
  const open = window.codync.appScheme === 'mexerica-open'
  const podeOcultar = local && managed
  // Hub antigo (sem `hiddenModelIds` na resposta): o botão fica desativado, com o motivo; nada é guardado só aqui.
  const hubOculta = account.hiddenModelIds !== undefined

  useEffect(() => {
    let live = true
    if (open || account.state !== 'connected' || !local) return
    contas.usage(account.id, store.spaceId).then((u) => { if (live) setUsage(u) }, () => { if (live) setUsage('erro') })
    return () => { live = false }
  }, [open, contas, account.id, account.state, local, store.spaceId])

  const act = async (work: () => Promise<unknown>, after?: () => void) => {
    setError(null)
    setComBots(false)
    try {
      await work()
      after?.()
    } catch (e) {
      setError(e)
    }
  }
  const reconnect: AccountAction | null = !local || !managed || (kind === 'api' && !store.isLoopback) ? null : kind === 'api' ? 'key' : account.state === 'disconnected' ? 'connect' : 'reconnect'
  // Pausada pelo dono volta com um gesto; a credencial ficou no cofre ou no perfil.
  const pausada = local && managed && account.paused === true && account.state === 'disconnected'
  const podeDesativar = local && managed && !open && account.paused !== undefined && (account.state === 'connected' || account.state === 'unavailable')

  return (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ModalHeader
        icon={<AgentTile registry={engineRegistry(account.backend)} icon={18} tile={32} radius={9} />}
        title={<bdi dir="auto">{view.title}</bdi>}
        subtitle={textos.ondeRoda(engineName(account.backend), store.hostName)}
        onClose={onClose}
        closeTitle={textos.fechar}
      />
      <div className="contas-panel-body contas-detalhe">
        <VStack gap={18}>
          <VStack gap={6}>
            <HStack gap={8}>
              <MarketBadge label={view.state} tone={view.tone} icon={view.tone === 'added' ? 'checkmark' : view.attention ? 'exclamationmark' : undefined} />
              {view.billing ? <Text size="caption" tone="secondary">{view.billing}</Text> : null}
              {viaProgramaOficial(account, kind) ? <MarketBadge label={textos.programaOficial} /> : null}
            </HStack>
            {viaProgramaOficial(account, kind) ? <Text size="caption" tone="secondary">{textos.programaOficialDetalhe}</Text> : null}
            <Text tone="secondary">{managed ? view.detail : textos.perfilAntigoDetalhe}</Text>
          </VStack>

          {renaming ? (
            <form className="contas-renomear" onSubmit={(e) => { e.preventDefault(); if (validLabel(label)) void act(() => contas.rename(account, label), () => setRenaming(false)) }}>
              <Text size="caption" weight="semibold" tone="secondary">{textos.renomearTitulo}</Text>
              <HStack gap={8}>
                <div className="contas-campo"><Field value={label} onChange={setLabel} label={textos.rotulo} placeholder={textos.rotulo} /></div>
                <button type="button" className="styled-button secondary" onClick={() => { setRenaming(false); setLabel(account.label) }}>{textos.cancelar}</button>
                <button type="submit" className="styled-button primary" disabled={!validLabel(label) || busy}>{textos.salvar}</button>
              </HStack>
              <Text size="caption" tone="secondary">{textos.rotuloDica}</Text>
            </form>
          ) : null}

          {open ? <Text size="caption" tone="secondary">{traduzir(idioma, 'contas.botsOpen')}</Text> : <section className="contas-secao" aria-label={textos.botsTitulo}>
            <HStack gap={6} className="contas-secao-titulo">
              <Text size="caption" weight="semibold" tone="secondary">{textos.botsTitulo}</Text>
              {account.botIds.length ? <Text size="caption" tone="tertiary" tabular>{formatarNumero(idioma, account.botIds.length)}</Text> : null}
            </HStack>
            {bots.length ? (
              <div className="contas-bots">
                {bots.map((bot) => (
                  <HStack key={bot.id} gap={10} className="contas-bot">
                    <BotAvatar bot={bot} size={30} animated={false} />
                    <VStack as="span" gap={1} grow shrink>
                      <Text weight="medium" truncate tone="text" dir="auto">{bot.name}</Text>
                      <Text size="caption" tone="secondary" truncate dir="auto" title={nomeModelo(bot.model)}>{nomeModelo(bot.model)}</Text>
                    </VStack>
                  </HStack>
                ))}
              </div>
            ) : account.botIds.length ? (
              <Text size="caption" tone="secondary">{textos.usadoPor(names)}</Text>
            ) : (
              <HStack gap={10} className="contas-bots-vazio">
                <Icon name="person.2" size={16} color="var(--text-tertiary, var(--ink-3))" />
                <VStack as="span" gap={2} grow shrink>
                  <Text tone="text">{textos.nenhumBot}</Text>
                  <Text size="caption" tone="secondary">{textos.botsVazioDetalhe}</Text>
                </VStack>
              </HStack>
            )}
          </section>}

          {!open ? <section className="contas-secao" aria-label={textos.modelosTitulo}>
            <HStack gap={6} className="contas-secao-titulo">
              <Text size="caption" weight="semibold" tone="secondary">{textos.modelosTitulo}</Text>
              <Spacer />
              {account.models.length ? <Text size="caption" tone="secondary" tabular>{textos.modelosResumo(visiveis, account.models.length)}</Text> : null}
            </HStack>
            {account.models.length ? (
              <>
                {/* Visíveis primeiro; os ocultos vão para o fim, num grupo com contador. Altura fixa, rola por dentro. */}
                <div className="contas-modelos-rolagem">
                {[account.models.filter((m) => !hidden.includes(m.id)), account.models.filter((m) => hidden.includes(m.id))].map((grupo, g) => grupo.length ? (
                  <VStack key={g} gap={6}>
                    {g === 1 ? (
                      <HStack gap={6} className="contas-modelos-grupo">
                        <Text size="caption" weight="semibold" tone="secondary">{textos.ocultosTitulo}</Text>
                        <Text size="caption" tone="tertiary" tabular>{formatarNumero(idioma, grupo.length)}</Text>
                      </HStack>
                    ) : null}
                    <ul className="contas-modelos">
                      {grupo.map((m) => {
                    const oculto = hidden.includes(m.id)
                    const nome = nomeModelo(m.id)
                    const usam = botNames(bots.filter((b) => b.model === m.id).map((b) => b.id), (id) => store.bots.get(id)?.name ?? null, idioma)
                    return (
                      <li key={m.id} className={`contas-modelo${oculto ? ' oculto' : ''}`}>
                        <VStack as="span" gap={1} grow shrink>
                          <HStack as="span" gap={6} shrink>
                            <Text tone="text" weight="medium" truncate dir="auto" title={nome}>{nome}</Text>
                            {oculto ? <MarketBadge label={textos.oculto} icon="eye.slash" /> : null}
                          </HStack>
                          <Text size="caption" tone="secondary" truncate>
                            {usam ? textos.modeloUsadoPor(usam) : <bdi dir="ltr">{m.id}</bdi>}
                          </Text>
                        </VStack>
                        {podeOcultar ? (
                          <button className="capsule-action contas-modelo-acao" disabled={busy || !hubOculta} title={hubOculta ? undefined : textos.ocultarSemHub} aria-label={`${oculto ? textos.mostrarRotulo(nome) : textos.ocultarRotulo(nome)}${hubOculta ? '' : `. ${textos.ocultarSemHub}`}`}
                            onClick={() => (!oculto && usam ? setOcultarEmUso({ id: m.id, nome, bots: usam }) : void act(() => contas.setModelHidden(account, m.id, !oculto)))}>
                            <Icon name={oculto ? 'eye' : 'eye.slash'} size={11} />
                            {oculto ? textos.mostrar : textos.ocultar}
                          </button>
                        ) : null}
                      </li>
                    )
                      })}
                    </ul>
                  </VStack>
                ) : null)}
                </div>
                {/* Conta antiga (não gerenciada) não oculta modelos: sem botão e sem a explicação. */}
                {managed ? <Text size="caption" tone="secondary">{podeOcultar && !hubOculta ? textos.ocultarSemHub : textos.modelosDica}</Text> : null}
              </>
            ) : (
              <HStack gap={10} className="contas-bots-vazio">
                <Icon name="exclamationmark.triangle" size={16} color="var(--text-attention, var(--attention))" />
                <VStack as="span" gap={2} grow shrink>
                  <Text tone="text">{textos.modelosVazioTitulo}</Text>
                  <Text size="caption" tone="secondary">{textos.modelosVazioDetalhe}</Text>
                </VStack>
                {local ? <button className="capsule-action" disabled={busy || contas.loading} onClick={() => onAction('refresh')}>{textos.modelosVerificar}</button> : null}
              </HStack>
            )}
          </section> : null}

          {account.state === 'connected' && local && !open ? <Uso usage={usage} /> : null}
          {account.state === 'connected' && local && managed && !open && account.backend !== 'grok' ? <SeEstaContaAcabar store={store} contas={contas} account={account} /> : null}
          {error || comBots ? <Text tone="danger" role="alert" className="selectable">{comBots ? textos.removerComBots(names) : errorMessage(error, idioma)}</Text> : null}
        </VStack>
      </div>
      <HStack gap={8} className="contas-footer">
        {local && managed ? (
          <>
            <button className="market-danger-button contas-perigo" disabled={busy} onClick={() => {
              if (account.botIds.length) setComBots(true)
              else setConfirm('remove')
            }}>{textos.remover}</button>
            {account.state !== 'disconnected' || viaProgramaOficial(account, kind) ? <Button kind="secondary" disabled={busy} onClick={() => setConfirm('disconnect')}>{textos.desconectar}</Button> : null}
            {podeDesativar ? <Button kind="secondary" disabled={busy} onClick={() => setConfirm('disable')}>{textos.desativar}</Button> : null}
            <Button kind="secondary" disabled={busy || renaming} onClick={() => { setLabel(account.label); setRenaming(true) }}>{textos.renomear}</Button>
          </>
        ) : null}
        <Spacer />
        {pausada ? (
          <Button disabled={busy} onClick={() => void act(() => contas.setEnabled(account, true))}>{textos.reativar}</Button>
        ) : reconnect ? (
          <Button disabled={busy} onClick={() => onAction(reconnect)}>
            {reconnect === 'key' ? (account.state === 'disconnected' ? textos.informarChave : textos.trocarChave) : reconnect === 'connect' ? textos.conectar : textos.reconectar}
          </Button>
        ) : view.primary?.action === 'refresh' ? (
          <Button onClick={() => onAction('refresh')}>{textos.tentarDeNovo}</Button>
        ) : <Button kind="secondary" onClick={onClose}>{textos.fechar}</Button>}
      </HStack>
      <Dialog
        open={confirm === 'disconnect'}
        title={textos.desconectarTitulo(account.label)}
        message={textos.desconectarMensagem}
        cancel={textos.cancelar}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirm(null)}
        actions={[{ title: textos.desconectar, destructive: true, action: () => void act(() => contas.disconnect(account)) }]}
      />
      {/* Pausar não sai da conta — a chave/login fica guardado; bots ligados ficam sem motor. */}
      <Dialog
        open={confirm === 'disable'}
        title={textos.desativarTitulo(account.label)}
        message={textos.desativarMensagem}
        cancel={textos.cancelar}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirm(null)}
        actions={[{ title: textos.desativar, action: () => void act(() => contas.setEnabled(account, false)) }]}
      />
      <Dialog
        open={ocultarEmUso !== null}
        title={ocultarEmUso ? textos.ocultarEmUsoTitulo(ocultarEmUso.nome) : ''}
        message={ocultarEmUso ? textos.ocultarEmUsoMensagem(ocultarEmUso.bots) : ''}
        cancel={textos.cancelar}
        language={superficieIdioma(idioma)}
        onClose={() => setOcultarEmUso(null)}
        actions={[{ title: textos.ocultar, action: () => { const alvo = ocultarEmUso; if (alvo) void act(() => contas.setModelHidden(account, alvo.id, true)) } }]}
      />
      <Dialog
        open={confirm === 'remove'}
        title={textos.removerTitulo(account.label)}
        message={textos.removerMensagem}
        cancel={textos.cancelar}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirm(null)}
        actions={[{ title: textos.remover, destructive: true, action: () => void act(() => contas.remove(account), onClose) }]}
      />
    </div></SheetPage>
  )
}

/** Uso só com origem comprovada pelo Hub; sem isso, indisponível (nunca a cota geral como se fosse desta conta). */
function Uso({ usage }: { usage: AccountUsage | null | 'erro' }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  if (usage === null) return <HStack gap={8} role="status"><Spinner size={12} /><Text size="caption" tone="secondary">{textos.uso}</Text></HStack>
  if (usage === 'erro' || usage.state === 'unavailable' || !usage.source || !usage.windows.length) {
    return (
      <VStack gap={4}>
        <Text size="caption" weight="semibold" tone="secondary">{textos.uso}</Text>
        <Text size="caption" tone="secondary">{textos.usoIndisponivel}</Text>
      </VStack>
    )
  }
  return (
    <VStack gap={8}>
      <Text size="caption" weight="semibold" tone="secondary">{textos.uso}</Text>
      {usage.windows.map((w) => {
        const percent = Math.max(0, Math.min(100, w.percent))
        const label = textos.usoJanela(w.id)
        const pct = formatarNumero(idioma, Math.round(percent) / 100, { style: 'percent' })
        // O Hub diz quando a janela está perto do limite (>= 90%); o app só mostra, sem recalcular o limiar.
        const perto = w.nearLimit === true
        return (
          <VStack key={w.id} gap={4}>
            <HStack gap={8}>
              <Text size="caption" tone="text">{label}</Text>
              <Spacer />
              <Text size="caption" tone={perto ? 'attention' : 'secondary'} weight={perto ? 'semibold' : undefined} tabular>{w.resetsAt ? traduzir(idioma, 'contas.usoValor', { pct: ltr(pct), quando: ltr(whenText(w.resetsAt, idioma)) }) : ltr(pct)}</Text>
            </HStack>
            <ProgressBar value={percent / 100} label={`${label}: ${pct}`} fill={perto ? 'var(--warning)' : undefined} />
            {perto ? (
              <HStack gap={4}>
                <Icon name="exclamationmark.triangle" size={11} color="var(--text-attention, var(--attention))" />
                <Text size="caption" tone="attention">{textos.usoPerto}</Text>
              </HStack>
            ) : null}
          </VStack>
        )
      })}
    </VStack>
  )
}
