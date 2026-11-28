// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { AtalhoBerga } from '../fork/atalho-berga/AtalhoBerga'
import { AtualizacaoAjuste } from '../fork/atualizacao/AtualizacaoAjuste'
import { IdiomaAjuste } from '../fork/idioma/IdiomaAjuste'
import type { Screen } from '@shared/analytics'
import { forkFeatures, showsAccountControls, showsComputerAccess } from '@shared/fork-features'
import { islandLabel } from '@shared/island'
import { Button, CardForm, CardSection, ChoicePicker, IconButton, ProgressBar, Switch, TabChoice, ValueRow } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { HStack, IconSlot } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useModel } from '../../lib/observable'
import { useScreenViewed } from '../../store/analytics'
import { useApp } from '../../store/context'
import { UpdateButton, useUpdates } from '../UpdateButton'
import { ComputersView } from './ComputersView'
import { EstadoPaginas, type EstadoGuardado } from './estado-pagina'
import { errorText, isMac } from './parts'
import { ComputerAccessView } from '../computer-access/ComputerAccessView'
import { ContasIaPage } from '../fork/contas-ia/ContasIaPage'
import { VozPage } from '../fork/voz/VozPage'
import { LugaresPage } from '../fork/onde-roda/LugaresPage'
import { FormSheet } from '../marketplace/pieces'
import { AnalyticsSheet } from './AnalyticsSheet'
import { AppearanceSettings } from './AppearanceSettings'
import { SidebarSettings } from './SidebarSettings'
import { ConversaSettings } from '../fork/conversa/ConversaSettings'
import { AccountSettingsCard } from '../fork/account/AccountSettingsCard'
import { BotDeFundoPage } from '../fork/bot-de-fundo/BotDeFundoPage'
import { MemoriaPage } from '../fork/memoria-ajustes/MemoriaPage'
import { CustoUso } from '../fork/custo/CustoUso'
import { abaDoUso } from '../fork/custo/modelo'
import type { BotStore, TeamSettings } from '../../store/bot-store'
import { abasDe, nomeDoSpaceAtivo, type SettingsPage } from '../fork/ajustes/tela'
import { RodapeCurto } from '../fork/ajustes/RodapeCurto'
import { ModoUsoAjuste } from '../fork/ajustes/ModoUsoAjuste'
import { AUTONOMIA_COMPLETO, AUTONOMIA_RESUMO } from '../fork/ajustes/textos'
import { ArmazenamentoPage } from '../fork/armazenamento/ArmazenamentoPage'
import { DelegacaoPage } from '../fork/delegacao/DelegacaoPage'
import { AtividadePage } from '../fork/atividade/AtividadePage'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { IconeApp } from '../fork/icone-app/IconeApp'
import { formatarData, type ChaveTexto } from '../../lib/fork-i18n/index.ts'
import { semParametros, Superficie } from './rotulo'
import { ltr } from '@shared/bidi'
import { capacidades } from '@shared/fork-capacidades'
import { abasVisiveis, paginaDisponivel, paginaDisponivelNaOpen } from '../fork/ajustes/gates-capacidades'

import { NuncaFazerPagina } from '../fork/nunca-fazer/NuncaFazer'
import { InstrucoesPage } from '../fork/instrucoes/InstrucoesPage'
export type { SettingsPage }

// TRAD-1 (D164/D165); D454 (fatia 4): a lateral de Ajustes no idioma escolhido, pelo catálogo (`ajustes.*`).
const thisComputer: ChaveTexto = isMac ? 'ajustes.grupo.esteMac' : 'ajustes.grupo.esteComputador'
const personal: ChaveTexto = 'ajustes.grupo.pessoal'
const pages: { id: SettingsPage; chave: ChaveTexto; /** O nome curto na lateral, quando o título é longo. */ curta?: ChaveTexto; group: ChaveTexto; icon: string }[] = [
  { id: 'general', chave: 'ajustes.pagina.geral', group: personal, icon: 'gearshape' },
  // D165: tema, destaque, fundo e vidro (docs/reference/redesenho/personalizacao.md).
  { id: 'appearance', chave: 'ajustes.pagina.aparencia', group: personal, icon: 'paintpalette' },
  // D165 fase C: ordem e visibilidade das seções da barra lateral.
  { id: 'sidebar', chave: 'ajustes.pagina.barraLateral', group: personal, icon: 'sidebar.left' },
  // DON-907: atalho de envio, menu / e o que mais for da conversa (fork/conversa).
  { id: 'conversation', chave: 'ajustes.pagina.conversa', group: personal, icon: 'bubble.left.and.bubble.right' },
  // D389: o modelo da memória (a mesma seleção do Bot de fundo) e o que os bots aprendem (fork/memoria-ajustes).
  { id: 'memory', chave: 'memoria.pagina', group: personal, icon: 'brain' },
  // MX-111 (fase 1): a lista Nunca fazer de cada Space, que todo bot dele lê (só orientação).
  { id: 'never-do', chave: 'nuncaFazer.pagina', group: personal, icon: 'hand.raised.slash' },
  // Instruções do dono: "Para todos os meus bots" e "Neste Space", somadas às instruções de cada bot.
  { id: 'instructions', chave: 'instrucoes.pagina', group: personal, icon: 'text.alignleft' },
  // D164: contas Codex/Claude/API do Hub, separadas da conta do app.
  { id: 'ai-accounts', chave: 'ajustes.pagina.contasIa', group: personal, icon: 'key' },
  // D371: regras de delegação do dono (fork/delegacao); a lista de modelos da S1 (D370) entra na mesma página.
  { id: 'delegation', chave: 'delegacao.pagina', group: personal, icon: 'arrow.triangle.branch' },
  { id: 'background', chave: 'ajustes.pagina.botDeFundo', group: personal, icon: 'sparkles' },
  // Entrega 4 (MX-7): o que o Bot de fundo fez, fora da página dele (fork/atividade).
  { id: 'activity', chave: 'ajustes.pagina.atividade', group: personal, icon: 'clock' },
  { id: 'workplaces', chave: 'ajustes.pagina.lugares', group: personal, icon: 'server.rack' },
  { id: 'computers', chave: 'ajustes.pagina.computadores', curta: 'ajustes.pagina.computadoresCurto', group: thisComputer, icon: 'desktopcomputer' },
  { id: 'access', chave: 'ajustes.pagina.acesso', group: thisComputer, icon: 'hand.raised' },
  // D457: só mostra o ícone da Mexerica (sem seletor; o D169 saiu).
  { id: 'app-icon', chave: 'aparencia.icone.pagina', group: thisComputer, icon: 'photo' },
  // voz-03 (D237): as escolhas de voz deste aparelho; as chaves de cada Hub local ou do túnel aparecem nela.
  { id: 'voice', chave: 'ajustes.pagina.voz', group: personal, icon: 'waveform' },
  // Plugins (WhatsApp) saiu daqui: é a quarta página de Extensões (revisão do app, Entrega 2; fork/extensoes).
  { id: 'usage', chave: 'ajustes.pagina.uso', group: personal, icon: 'gauge.with.dots.needle.33percent' },
  // DON-905 (C6): o espaço do Hub, só leitura e só do dono.
  { id: 'storage', chave: 'ajustes.pagina.armazenamento', group: personal, icon: 'internaldrive' },
  { id: 'updates', chave: 'ajustes.pagina.atualizacoes', group: thisComputer, icon: 'arrow.down.circle' },
]

/** Each page as `screen_viewed` names it; Computer access and Voice chat report themselves. */
const pageScreens: Record<SettingsPage, Screen | null> = { general: 'settings', appearance: 'settings', sidebar: 'settings', conversation: 'settings', memory: 'settings', 'never-do': 'settings', instructions: 'settings', 'ai-accounts': null, delegation: 'settings', background: 'settings', activity: 'settings', workplaces: null, updates: 'settings', computers: 'computers', usage: 'usage', storage: 'settings', access: null, voice: null, 'app-icon': 'settings' }

/**
 * The chat window's Settings: pages in a sidebar, ChatGPT's settings layout. ajustes-tela-01: the body of the
 * Ajustes screen (fork/ajustes/AjustesTela), which owns the page and tab; long pages show tabs above the
 * page, and only the page scrolls.
 */
export function SettingsView({ page: requested, aba: abaPedida, onPage, onAba }: { page: SettingsPage; aba: string | null; onPage: (page: SettingsPage) => void; onAba: (aba: string) => void }) {
  const app = useApp()
  // Fork: páginas cortadas (Updates; Computer access sem host local, D76) não aparecem nem abrem: caem em Geral.
  const cap = capacidades(window.codync)
  const open = window.codync.appScheme === 'mexerica-open'
  const shownPages = pages.filter((p) => (open ? paginaDisponivelNaOpen(p.id) : p.id !== 'access' || showsComputerAccess(app.host.state)) &&
    (p.id !== 'updates' || forkFeatures.updates) && paginaDisponivel(p.id, cap))
  const page = shownPages.some((p) => p.id === requested) ? requested : 'general'
  const t = useT()
  const idioma = useIdioma()
  const tx = semParametros(t)
  const titulo = (p: (typeof pages)[number]) => tx(p.chave)
  const current = pages.find((p) => p.id === page)!
  const abas = page === 'ai-accounts' && open
    ? undefined : abasVisiveis(page, abasDe[page], cap, app.managedStores.length)
  // Uma aba que este aparelho não mostra (ex.: Chaves na Web) cai na primeira, nunca numa página vazia.
  const aba = abas && abaPedida && !abas.some((a) => a.id === abaPedida) ? abas[0].id : abaPedida
  // O cabeçalho do primeiro grupo é o Space ativo; "Pessoal" só quando ele é o ativo (nenhum Space aberto).
  const ativo = app.spaceMirrors.active
  const nomeDoSpace = nomeDoSpaceAtivo(ativo, ativo ? app.spaceMirrors.root(ativo.computerId)?.spaces?.spaces : undefined)
  // Só a página ativa fica montada: trocar de página desmonta a anterior, e o DOM de Ajustes não cresce com as
  // visitadas. A aba de cada página já fica na AjustesTela. O scroll fica aqui, porque a página sai do DOM e
  // Chromium só o guardava enquanto ela ficava escondida; ao montar, volta à posição que tinha.
  // Rascunhos e escolhas das páginas (estado-pagina.ts): o mapa vive enquanto Ajustes está aberto.
  const [guardado] = useState<EstadoGuardado>(() => new Map())
  const rolagens = useRef<Partial<Record<SettingsPage, number>>>({})
  const paginaRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => { if (paginaRef.current) paginaRef.current.scrollTop = rolagens.current[page] ?? 0 }, [page])
  const abaAtual = abas?.find((a) => a.id === aba)
  useScreenViewed(pageScreens[page])
  return (
    <div className={`settings${page === 'appearance' ? ' settings-appearance' : ''}`}>
      <nav className="settings-sidebar idioma-superficie" {...superficieIdioma(idioma)} aria-label={t('ajustes.paginas')}>
        {[personal, thisComputer].filter((group) => shownPages.some((p) => p.group === group)).map((group) => (
          <Fragment key={group}>
            <div className="settings-group">{group === personal && nomeDoSpace ? nomeDoSpace : tx(group)}</div>
            {shownPages.filter((p) => p.group === group).map((item) => (
              <button key={item.id} className={`settings-item ${item.id === page ? 'selected' : ''}`} aria-current={item.id === page ? 'page' : undefined} onClick={() => onPage(item.id)}>
                <IconSlot width={18} tone="secondary">
                  <Icon name={item.icon} size={13} />
                </IconSlot>
                <Text truncate grow>{item.curta ? tx(item.curta) : titulo(item)}</Text>
                {item.id === 'computers' && app.approvals.length > 0 ? (
                  <Text size="caption" weight="semibold" tone="secondary">{app.approvals.length}</Text>
                ) : null}
              </button>
            ))}
          </Fragment>
        ))}
      </nav>
      <section className="settings-main" aria-labelledby="settings-title">
        <div className="settings-head idioma-superficie" {...superficieIdioma(idioma)}>
          <h2 id="settings-title" className="settings-title">{titulo(current)}</h2>
          {abas && aba ? <TabChoice label={t('ajustes.partesDe', { pagina: titulo(current) })} selection={aba} options={abas.map((a) => ({ id: a.id, label: tx(a.chave) }))} onChange={onAba} /> : null}
        </div>
        <div key={page} ref={paginaRef} className="settings-page" role={abas ? 'tabpanel' : undefined} aria-label={abaAtual ? tx(abaAtual.chave) : undefined} onScroll={(e) => { rolagens.current[page] = e.currentTarget.scrollTop }}>
          <EstadoPaginas.Provider value={guardado}>
            <PageContent page={page} aba={aba} onPage={onPage} />
          </EstadoPaginas.Provider>
        </div>
      </section>
    </div>
  )
}

function PageContent({ page, aba, onPage }: { page: SettingsPage; aba: string | null; onPage: (page: SettingsPage) => void }) {
  const app = useApp()
  const openVoice = window.codync.appScheme === 'mexerica-open' ? undefined : () => onPage('voice')
  switch (page) {
    case 'access':
      return <ComputerAccessView />
    case 'general':
      return <GeneralSettings />
    case 'appearance':
      return <AppearanceSettings />
    case 'sidebar':
      return <SidebarSettings />
    case 'conversation':
      return <ConversaSettings />
    case 'memory':
      return <MemoriaPage />
    case 'never-do':
      return <NuncaFazerPagina />
    case 'instructions':
      return <InstrucoesPage />
    case 'ai-accounts':
      return <ContasIaPage aba={aba === 'contas' || aba === 'motores' ? aba : null} />
    case 'delegation':
      return <DelegacaoPage />
    case 'background':
      return <BotDeFundoPage openVoice={openVoice} openActivity={() => onPage('activity')} />
    case 'activity':
      return <AtividadePage openBackground={() => onPage('background')} />
    case 'workplaces':
      return <LugaresPage />
    case 'computers':
      return <ComputersView />
    case 'voice':
      return <VozPage reserva={app.selectedStore} aba={aba === 'ouvir' || aba === 'vivo' || aba === 'chaves' ? aba : null} />
    case 'usage':
      return (
        <CardForm>
          <CustoUso aba={abaDoUso(aba)} />
        </CardForm>
      )
    case 'app-icon':
      return <IconeApp />
    case 'storage':
      return <ArmazenamentoPage />
    case 'updates':
      return <UpdateSettings />
  }
}

/** Account, window and app preferences. */
export function GeneralSettings() {
  const app = useApp()
  const t = useT()
  const cap = capacidades(window.codync)
  const open = window.codync.appScheme === 'mexerica-open'
  const [showInDock, setShowInDock] = useState(true)
  // A página monta a cada visita: lê estas chaves ao abrir, porque a bandeja também as muda.
  useEffect(() => { if (cap.dock) void window.codync.app.showInDock().then(setShowInDock) }, [cap.dock])
  const [launchAtLogin, setLaunchAtLogin] = useState(false)
  useEffect(() => { if (cap.launchAtLogin) void window.codync.app.launchAtLogin().then(setLaunchAtLogin) }, [cap.launchAtLogin])
  const [island, setIsland] = useState(false)
  const islandSupported = window.codync.island.supported
  useEffect(() => { if (islandSupported) void window.codync.island.enabled().then(setIsland) }, [islandSupported])

  return (
    <CardForm>
      {/* D171 (Clerk 6): conta e saída não confirmada em views/fork/account. */}
      {!open && showsAccountControls() && <AccountSettingsCard />}
      <ModoUsoAjuste />
      {!open && app.managedStores.map((store) => <TeamAutonomy key={store.computer.id} store={store} />)}
      <IdiomaAjuste />
      <Superficie>
      {!open && <AtalhoBerga />}
      {cap.dock || islandSupported || cap.launchAtLogin ? <CardSection title={t('ajustes.geral.sistema')}>
        {cap.dock ? <Switch on={showInDock} onChange={(on) => void window.codync.app.setShowInDock(on).then(setShowInDock)}>{t('ajustes.geral.dock')}</Switch> : null}
        {islandSupported ? (
          <Switch on={island} onChange={(on) => void window.codync.island.setEnabled(on).then(setIsland)}>
            {t(islandLabel(window.codync.platform))}
          </Switch>
        ) : null}
        {cap.launchAtLogin ? (
          <Switch on={launchAtLogin} onChange={(on) => void window.codync.app.setLaunchAtLogin(on).then(setLaunchAtLogin)}>
            {t('ajustes.geral.aoIniciar')}
          </Switch>
        ) : null}
      </CardSection> : null}
      </Superficie>
      <AtualizacaoAjuste />
      {!open && forkFeatures.analytics && <PrivacySettings />}
    </CardForm>
  )
}

/** A autonomia geral é uma escolha do usuário no host, inclusive por túnel SSH. */
function TeamAutonomy({ store: source }: { store: BotStore }) {
  const store = useModel(source)
  const t = useT()
  const idioma = useIdioma()
  const online = store.connection.kind === 'online'
  const [permission, setPermission] = useState<TeamSettings['permission']>()
  const [changing, setChanging] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    setPermission(undefined)
    if (online) void store.teamSettings()
      .then((settings) => { if (live) { setPermission(settings.permission); setError(null) } })
      .catch((e: unknown) => { if (live) setError(errorText(e)) })
    return () => { live = false }
  }, [store, online])
  return (
    <Superficie>
    <CardSection title={t('ajustes.autonomia.titulo')} footer={<RodapeCurto resumo={t(AUTONOMIA_RESUMO)} completo={t(AUTONOMIA_COMPLETO)} />}>
      <ValueRow label={t('ajustes.autonomia.gerenciar')}>
        <ChoicePicker language={superficieIdioma(idioma)}
          selection={permission ?? 'ask'}
          options={[{ id: 'ask', label: t('ajustes.autonomia.pedir') }, { id: 'auto', label: t('ajustes.autonomia.liberar') }]}
          disabled={!online || changing || permission === undefined}
          onChange={(value) => {
            setChanging(true)
            setError(null)
            void store.setTeamSettings({ permission: value })
              .then((settings) => setPermission(settings.permission))
              .catch((e: unknown) => setError(errorText(e)))
              .finally(() => setChanging(false))
          }}
        />
      </ValueRow>
      {error ? <Text size="footnote" tone="danger" dir="auto">{error}</Text> : null}
    </CardSection>
    </Superficie>
  )
}

/** Product analytics on this computer's host; only the computer itself may change it. */
function PrivacySettings() {
  const app = useApp()
  const local = useModel(app.local)
  const running = local !== null && local.connection.kind === 'online'
  const [about, setAbout] = useState(false)
  const t = useT()
  const idioma = useIdioma()
  const setShared = (on: boolean) => void local?.setAnalytics(on).catch((e: unknown) => app.setError(errorText(e)))
  return (
    <Superficie>
      <CardSection title={t('ajustes.privacidade.titulo')} footer={t('ajustes.privacidade.rodape')}>
        <HStack gap={4}>
          <span className={running ? undefined : 'settings-off'}>{t('ajustes.privacidade.compartilhar')}</span>
          <IconButton title={t('ajustes.privacidade.sobre')} icon="info.circle" onClick={() => setAbout(true)} />
          <Switch on={local?.analytics === true} onChange={setShared} disabled={!running} label={t('ajustes.privacidade.compartilhar')} className="settings-privacy-switch" />
        </HStack>
      </CardSection>
      <FormSheet language={superficieIdioma(idioma)} open={about} onClose={() => setAbout(false)}>
        <AnalyticsSheet />
      </FormSheet>
    </Superficie>
  )
}

/** The app's update check and preferences, the same as the menu bar's Updates menu. */
function UpdateSettings() {
  const updates = useUpdates()
  const t = useT()
  const idioma = useIdioma()
  if (!updates || !forkFeatures.updates) return null
  const footer = updates.supported ? null : t('ajustes.atualizacoes.soRelease')
  const lastChecked = updates.lastCheck
    ? t('ajustes.atualizacoes.ultima', { quando: formatarData(idioma, updates.lastCheck, { dateStyle: 'medium', timeStyle: 'short' }) })
    : null
  return (
    <CardForm>
      <Superficie>
      <CardSection title={t('marca.ajustes.secaoApp')} footer={footer}>
        {[
          <ValueRow key="version" label={t('ajustes.atualizacoes.versao')} detail={lastChecked}>
            <HStack as="span" gap={10}>
              <span className="bidi-isolado" dir="ltr">{window.codync.appVersion}</span>
              <UpdateButton updates={updates} />
            </HStack>
          </ValueRow>,
          ...(updates.downloadPercent !== null
            ? [
                <div key="progress" className="settings-row settings-appear">
                  <ProgressBar taskProgress value={updates.downloadPercent / 100} label={t('marca.atualizar.baixando', { versao: ltr(updates.availableVersion ?? '') })} />
                </div>,
              ]
            : []),
          <Switch key="check" on={updates.autoCheck} onChange={(on) => window.codync.updates.setAutoCheck(on)} disabled={!updates.supported}>
            {t('ajustes.atualizacoes.verificar')}
          </Switch>,
          <Switch key="download" on={updates.autoDownload} onChange={(on) => window.codync.updates.setAutoDownload(on)} disabled={!updates.supported}>
            {t('ajustes.atualizacoes.baixar')}
          </Switch>,
          ...(updates.error
            ? [
                <div key="error" className="settings-row settings-appear">
                  <Text tone="attention" grow dir="auto">{updates.error}</Text>
                  <Button kind="secondary" onClick={() => window.codync.updates.check()} disabled={updates.checking}>
                    {t('ajustes.atualizacoes.tentar')}
                  </Button>
                </div>,
              ]
            : []),
        ]}
      </CardSection>
      </Superficie>
    </CardForm>
  )
}
