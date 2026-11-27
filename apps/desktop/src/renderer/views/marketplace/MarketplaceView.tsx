// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Button, DropdownMenu, IconButton, Spinner, TabChoice } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, SheetPage, useDismiss } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { prefs, usePref } from '../../lib/prefs'
import { useStore } from '../../store/context'
import { useScreenViewed } from '../../store/analytics'
import { ComputerBadge } from '../ComputerBadge'
import { CredentialsView } from './CredentialsView'
import { CustomConnectorSheet } from './ConnectorSheets'
import { connectorConfiguration, errorText, needsSignIn } from './market-models'
import { EmptyState, FormSheet, ItemGrid, ItemRow, MarketBadge, MarketRow, MarketSearch, MarketSection, ScreenHeader, SkeletonGrid, SlideStack, TileIcon } from './pieces'
import { PaginaSkills } from '../fork/catalogo-skills/PaginaSkills'
import { extensoesSomenteCatalogo } from '../fork/extensoes/tela'
import { PaginaConectores } from '../fork/mercado/PaginaConectores'
import { SkillsInstaladas } from '../fork/catalogo-skills/SkillsInstaladas'
import { detalheDe, filtrarLocal, selosDe, type Detalhe } from '../fork/meus-mcp/estado'
import { usePlugins } from './plugins'
import { LogoConector } from '../fork/extensoes/LogoConector'
import { LigarEmTodos } from '../fork/extensoes/LigarEmTodos'
import type { ChaveTexto } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

type MarketTab = 'apps' | 'connectors' | 'skills'
type ApplicationTab = 'apps' | 'connectors'
type ExtensionTab = 'applications' | 'skills'

// D169 (28/10, pedido do dono): no Agentes tab. The agents (ACP harnesses) are managed in Ajustes → Contas de IA
// (views/fork/contas-ia), the one place; this screen kept a duplicate catalog of them.
// D454 (fatia 8a): as abas levam só as chaves do catálogo; a tela traduz no idioma escolhido.
const EXTENSION_TABS: { id: ExtensionTab; chave: ChaveTexto }[] = [
  { id: 'applications', chave: 'extensoes.pagina.apps' },
  { id: 'skills', chave: 'extensoes.pagina.skills' },
]

const TABS: { id: MarketTab; chave: ChaveTexto; search: ChaveTexto; source: ChaveTexto }[] = [
  { id: 'connectors', chave: 'extensoes.meusMcp', search: 'extensoes.busca.mcp', source: 'marca.apps.meusMcp' },
  { id: 'skills', chave: 'extensoes.pagina.skills', search: 'extensoes.busca.skills', source: 'extensoes.fonte.skills' },
]

/** As chaves das abas não têm parâmetros: o tradutor tipado as recebe só pela chave. */
const semParametros = (t: ReturnType<typeof useT>) => t as unknown as (chave: ChaveTexto) => string

const noSearch: Record<MarketTab, string> = { apps: '', connectors: '', skills: '' }

/**
 * Extensões: Aplicativos (Donna Apps e Meus MCP) e Skills (D104); os agentes ficam em Ajustes → Contas de IA.
 * No Donnabot há um Hub só (D76); conectores/contas começam desligados por bot (D91). Quando o app
 * conhece mais de um computador, o cabeçalho troca entre os online (the caller swaps the store).
 */
export function MarketplaceView({ computers, knownComputers, computer, onComputer, embedded, ativa = true, pedidoAdicionar = 0 }: {
  /** Os computadores online, para onde dá para trocar. */
  computers: { id: string; name: string }[]
  /** Quantos computadores o app conhece, online ou não. */
  knownComputers: number
  computer: string
  onComputer: (id: string) => void
  /** D249 E3: inside the Extensões screen (fork/extensoes), one page, without this sheet's title, tabs and Fechar. */
  embedded?: 'connectors' | 'skills'
  /** Em Extensões a instância fica montada entre as páginas; `false` enquanto outra página (Plugins, Credenciais) está na frente. */
  ativa?: boolean
  /** Pedido de "Adicionar conector personalizado" vindo do cabeçalho da página (muda a cada clique): abre a folha. */
  pedidoAdicionar?: number
}) {
  const store = useStore()
  const t = useT()
  const tx = semParametros(t)
  const idioma = useIdioma()
  const plugins = usePlugins(store)
  const dismiss = useDismiss()
  useScreenViewed('marketplace')
  const [savedTab, setSavedTab] = usePref(prefs.marketplaceTab)
  const [applicationTab, setApplicationTab] = useState<ApplicationTab>(() =>
    savedTab === 'apps' || savedTab === 'connectors' ? savedTab : prefs.marketplaceApplicationTab.get(),
  )
  useEffect(() => { prefs.marketplaceApplicationTab.set(applicationTab) }, [applicationTab])
  const tabs = TABS
  const tab = embedded ? (TABS.find((a) => a.id === embedded) ?? TABS[0]) : (tabs.find((a) => a.id === savedTab) ?? tabs[0])
  const applicationTabs = tabs.filter((a) => a.id === 'connectors')
  const selectedApplication = applicationTabs.find((a) => a.id === applicationTab) ?? applicationTabs[0]
  const extension: ExtensionTab = tab.id === 'skills' ? 'skills' : 'applications'
  // A saved Agentes (the removed tab, or an old link to it) opens Aplicativos, and the preference is fixed for good.
  useEffect(() => { if (savedTab === 'agents') setSavedTab(selectedApplication.id) }, [savedTab])
  // Os IDs antigos continuam válidos na preferência e nas buscas de cada aba.
  const selectTab = (id: MarketTab) => {
    if (id === 'apps' || id === 'connectors') setApplicationTab(id)
    setSavedTab(id)
  }
  /** Cada aba guarda a sua busca; o campo do topo mostra a da aba aberta. */
  const [searches, setSearches] = useState(noSearch)
  const search = searches[tab.id]
  const setSearch = (value: string) => setSearches((old) => ({ ...old, [tab.id]: value }))
  const [addingConnector, setAddingConnector] = useState(false)
  /** Each opening of Adicionar MCP starts a fresh form (its outcome belongs to that gesture). */
  const [addKey, setAddKey] = useState(0)
  const openAdd = () => {
    setAddKey((k) => k + 1)
    setAddingConnector(true)
  }
  // Na página nova de Conectores o botão abre a tela própria (PaginaConectores); a folha antiga fica para quem não a usa.
  useEffect(() => { if (pedidoAdicionar > 0 && !mercado) openAdd() }, [pedidoAdicionar])
  const [showInstalled, setShowInstalled] = useState(false)
  const [showCredentials, setShowCredentials] = useState(false)

  // Em Extensões a mesma instância serve Aplicativos, Conectores e Skills (sem remontar nem buscar o catálogo de
  // novo). O que o remount fazia fica aqui: ao trocar de página ou sair da frente, as folhas fecham e a rolagem volta
  // ao topo. A busca já é uma por página.
  const rolagem = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    setAddingConnector(false)
    setShowInstalled(false)
    setShowCredentials(false)
    rolagem.current?.scrollTo(0, 0)
    rolagem.current?.closest('.settings-page')?.scrollTo(0, 0)
  }, [embedded, ativa])

  const query = search.trim()
  // Revisão do app (Entrega 2): cada página de Extensões mostra só o que é dela. Aplicativos e Conectores contam
  // os conectores; Skills já lista as próprias instaladas (PaginaSkills) e fica sem este cabeçalho.
  const installedCount = plugins.installedConnectors.length + (embedded ? 0 : plugins.installedSkills.length)
  // Conectores (Marketplace, fatia 1b) tem cabeçalho e busca próprios (views/fork/mercado); só a troca de computador fica.
  const mercado = embedded === 'connectors'
  const semCabecalho = (embedded === 'skills' || mercado) && knownComputers <= 1

  // D175: the installed list is read again whenever the Hub, link or generation changes (a read, never a
  // write); until then nothing from the previous origin shows.
  const hub = store.hello?.hostId ?? null
  const link = store.connectedClient
  const epoch = store.principalEpoch
  useEffect(() => { if (!extensoesSomenteCatalogo()) void plugins.refresh() }, [hub, link, epoch])

  const header = (
    <div className="market-heading">
      <HStack gap={10} shrink>
        <ComputerBadge computer={store.computer} size={32} />
        <VStack shrink className="market-heading-titles">
          {embedded ? null : <Text size="title" weight="semibold" tone="text">{t('extensoes.titulo')}</Text>}
          {/* Um Hub só (D76), com acessos por bot (D91). A troca só aparece quando o app conhece mais de um computador. */}
          {knownComputers > 1 ? (
            <DropdownMenu
              title={t('extensoes.trocarComputador')}
              className="market-computer-switch"
              language={superficieIdioma(idioma)}
              items={() => computers.map((c) => ({ title: isolado(c.name), selected: c.id === computer, action: () => onComputer(c.id) }))}
            >
              <Text truncate dir="auto" aria-label={t('extensoes.computador', { nome: isolado(store.hostName) })}>{store.hostName}</Text>
              <Icon name="chevron.down" size={10} weight="semibold" />
            </DropdownMenu>
          ) : (
            <Text size="subheadline" tone="secondary" truncate>{t('extensoes.hub')}</Text>
          )}
        </VStack>
      </HStack>
      <Spacer />
      {/* Revisão do app (Entrega 2): dentro de Extensões, Credenciais é uma página da lista lateral, não um botão aqui. */}
      {embedded ? null : <Button kind="secondary" onClick={() => setShowCredentials(true)}><Icon name="lock.shield" size={12} />{t('extensoes.credenciais')}</Button>}
      {installedCount > 0 && embedded !== 'skills' ? (
        <button className="press market-installed" onClick={() => setShowInstalled(true)}>
          <HStack as="span" align="stretch" className="market-installed-logos">
            {plugins.installedConnectors.slice(0, 3).map((c) => (
              <HStack as="span" align="stretch" key={c.id}>
                <LogoConector conector={c} size={26} />
              </HStack>
            ))}
          </HStack>
          <VStack as="span" align="flex-start" className="market-installed-text">
            <Text tone="secondary">{t.plural('extensoes.instalados', installedCount)}</Text>
            <Text size="caption" tone="secondary">{t(embedded ? 'extensoes.pagina.connectors' : 'extensoes.conectoresESkills')}</Text>
          </VStack>
          <Icon name="chevron.right" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />
        </button>
      ) : null}
    </div>
  )

  const searchField = (
    <MarketSearch
      value={search}
      placeholder={tx(tab.search)}
      onChange={setSearch}
    />
  )

  const connectorsTab = (
    <>
      <MarketSection title={query ? t('extensoes.seusMcpCom', { busca: isolado(query) }) : t('extensoes.meusMcp')}>
        <NativeConnectorList query={query} />
      </MarketSection>
      <MarketSection title={t('extensoes.adicionar')}>
        <ItemGrid>
          <MarketRow
            title={t('extensoes.adicionarMcp')}
            subtitle={t('extensoes.adicionarMcpDetalhe')}
            added={false}
            addLabel={t('extensoes.novo')}
            icon={<TileIcon name="point.3.connected.trianglepath.dotted" />}
            onAdd={openAdd}
          />
        </ItemGrid>
      </MarketSection>
    </>
  )

  // D166: a página Skills (views/fork/catalogo-skills), com cabeçalho, busca e rodapé próprios, no idioma escolhido
  // (fatia 8b): a raiz dela declara lang/dir.
  const skillsTab = <PaginaSkills busca={search} onBusca={setSearch} />

  const page = (
    <div className={`market-page idioma-superficie${embedded ? ' market-embedded' : ''}`} {...superficieIdioma(idioma)}>
      <div className="market-scroll" ref={rolagem}>
        <div className="market-page-content">
          {semCabecalho ? null : header}
          {embedded ? null : <TabChoice label={t('extensoes.titulo')} selection={extension} options={EXTENSION_TABS.map((a) => ({ id: a.id, label: tx(a.chave) }))} onChange={(id) => selectTab(id === 'applications' ? selectedApplication.id : id)} />}
          {!embedded && extension === 'applications' ? (
            <TabChoice label={t('extensoes.pagina.apps')} selection={tab.id} options={applicationTabs.map((a) => ({ id: a.id, label: tx(a.chave) }))} onChange={selectTab} />
          ) : null}
          {tab.id === 'skills' || mercado ? null : searchField}
          {mercado ? <PaginaConectores ativa={ativa} pedidoAdicionar={pedidoAdicionar} /> : tab.id === 'connectors' ? connectorsTab : tab.id === 'skills' ? skillsTab : null}
          {tab.id === 'skills' || mercado ? null : <Text size="footnote" tone="tertiary">{tx(tab.source)} {t('extensoes.fonte.ficaConectores')}</Text>}
        </div>
      </div>
      {/* Dismissing slides the sheet away; clearing the caller's state would cut it off. */}
      {embedded ? null : <div className="market-close">
        <IconButton title={t('extensoes.fechar')} icon="xmark" onClick={dismiss} />
      </div>}
    </div>
  )

  return (
    <>
      <SlideStack base={page} top={showCredentials ? <CredentialsView onBack={() => setShowCredentials(false)} /> : showInstalled ? <InstalledView back={() => setShowInstalled(false)} soConectores={!!embedded} /> : null} />
      <FormSheet language={superficieIdioma(idioma)} open={addingConnector} onClose={() => setAddingConnector(false)}>
        <CustomConnectorSheet key={addKey} />
      </FormSheet>
    </>
  )
}

/**
 * Meus MCP (D175, docs/features/marketplace.md "Na tela"): the native configurations of the Hub, link and
 * generation of now, with honest badges; the search only filters this list. No catalog, no pages.
 */
function NativeConnectorList({ query }: { query: string }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const plugins = usePlugins(store)
  const [signingIn, setSigningIn] = useState<string | null>(null)
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null)
  const state = plugins.connectorsState
  const error = plugins.connectorsError
  const all = plugins.nativeConnectors
  const items = filtrarLocal(all, query)

  const signIn = async (id: string) => {
    setSigningIn(id)
    try {
      await plugins.signIn(id)
    } catch (e) {
      store.setError(errorText(e))
    }
    setSigningIn(null)
  }
  const remove = async (id: string) => {
    try {
      await plugins.removeConnector(id)
    } catch (e) {
      store.setError(errorText(e))
    }
  }

  if (state === 'semConexao') return <EmptyState icon="wifi.slash" title={t('extensoes.semConexao')} detail={t('extensoes.mcpVoltam')} />
  if (state === 'lendo') return <SkeletonGrid />
  const retry = <Button kind="secondary" onClick={() => void plugins.refresh()}>{t('extensoes.tentarDeNovo')}</Button>
  // A failed read is an error, never "no MCP": a list of this same origin may stay in view under it.
  if (error && all.length === 0) return <EmptyState icon="exclamationmark.triangle" title={t('extensoes.erroLerMcp')} detail={error} action={retry} />
  // No second opener here: the first MCP saved replaces this state, and a button inside it would be gone
  // when the form closes, leaving focus on the page. Adicionar MCP, just below, is the one opener, and
  // the form gives focus back to it (D175 focus check).
  if (all.length === 0) {
    return <EmptyState toque icon="point.3.connected.trianglepath.dotted" title={t('extensoes.nenhumMcp')} detail={t('extensoes.nenhumMcpDetalhe')} />
  }
  return (
    <>
      {error ? (
        <HStack gap={8}>
          <Text size="footnote" tone="danger" grow>{t('extensoes.erroAtualizar', { erro: isolado(error) })}</Text>
          {retry}
        </HStack>
      ) : null}
      {items.length === 0 ? (
        <EmptyState toque icon="magnifyingglass" title={t('extensoes.nenhumMcpEncontrado')} detail={t('extensoes.nadaComMcp', { busca: isolado(query) })} />
      ) : (
        <ItemGrid>
          {items.map((c) => (
            <div key={c.id} className="eet-item">
            <ItemRow
              title={c.name}
              subtitle={textoDoDetalhe(detalheDe(c), t)}
              subtitleCodigo={'codigo' in detalheDe(c)}
              icon={<LogoConector conector={c} />}
              badges={selosDe(c).map((s) => <MarketBadge key={s.id} label={t(s.chave)} icon={s.icone} />)}
              trailing={(
                <>
                  {signingIn === c.id ? <Spinner /> : needsSignIn(c) ? <Button onClick={() => void signIn(c.id)}>{t('extensoes.entrar')}</Button> : null}
                  <RemoveButton title={c.name} onClick={() => setRemoving({ id: c.id, name: c.name })} />
                </>
              )}
            />
            {/* MX-264: a prévia abre embaixo da linha; sem alvo salvo (manual) não há o que ligar. */}
            {connectorConfiguration(c) === 'manualRequired' ? null : <LigarEmTodos tipo="conector" id={c.id} nome={c.name} />}
            </div>
          ))}
        </ItemGrid>
      )}
      <Dialog
        open={removing !== null}
        title={t('extensoes.removerTitulo', { nome: isolado(removing?.name ?? '') })}
        message={removing ? t('extensoes.removerMcpMensagem') : null}
        actions={removing ? [{ title: t('extensoes.remover'), destructive: true, action: () => void remove(removing.id) }] : []}
        cancel={t('extensoes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setRemoving(null)}
      />
    </>
  )
}

// MARK: - Installed

function InstalledView({ back, soConectores = false }: { back: () => void; /** Dentro de Extensões: só conectores; as skills instaladas ficam na página Skills. */ soConectores?: boolean }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const plugins = usePlugins(store)
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null)
  const [signingIn, setSigningIn] = useState<string | null>(null)
  // D175: an unread, failed or offline list is never "Nada instalado" (another origin's never shows).
  const connectorsState = plugins.connectorsState
  const installedSkills = soConectores ? [] : plugins.installedSkills
  const empty = connectorsState === 'lida' && plugins.installedConnectors.length === 0 && installedSkills.length === 0

  const signIn = async (id: string) => {
    setSigningIn(id)
    try {
      await plugins.signIn(id)
    } catch (e) {
      store.setError(errorText(e))
    }
    setSigningIn(null)
  }

  const remove = async (r: { id: string }) => {
    try {
      await plugins.removeConnector(r.id)
    } catch (e) {
      store.setError(errorText(e))
    }
  }

  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ScreenHeader title={t(soConectores ? 'extensoes.pagina.connectors' : 'extensoes.conectoresESkills')} onBack={back} />
      <div className="market-installed-body">
        <div className="market-scroll">
          <div className="market-page-content">
            {empty ? <EmptyState toque icon="shippingbox" title={t('extensoes.nadaInstalado')} detail={t('extensoes.nadaInstaladoDetalhe')} /> : null}
            {connectorsState === 'semConexao' ? <EmptyState icon="wifi.slash" title={t('extensoes.semConexao')} detail={t('extensoes.conectoresVoltam')} /> : null}
            {connectorsState === 'lendo' ? <SkeletonGrid /> : null}
            {connectorsState === 'erro' ? (
              <EmptyState
                icon="exclamationmark.triangle"
                title={t('extensoes.erroLerConectores')}
                detail={plugins.connectorsError ?? ''}
                action={<Button kind="secondary" onClick={() => void plugins.refresh()}>{t('extensoes.tentarDeNovo')}</Button>}
              />
            ) : null}
            {plugins.installedConnectors.length ? (
              <MarketSection title={t('extensoes.pagina.connectors')}>
                <ItemGrid>
                  {plugins.installedConnectors.map((c) => (
                    <ItemRow
                      key={c.id}
                      title={c.name}
                      subtitle={needsSignIn(c) ? t('extensoes.entreParaUsar') : textoDoDetalhe(detalheDe(c), t)}
                      subtitleCodigo={!needsSignIn(c) && 'codigo' in detalheDe(c)}
                      icon={<LogoConector conector={c} />}
                      trailing={(
                        <>
                          {signingIn === c.id ? <Spinner /> : needsSignIn(c) ? <Button onClick={() => void signIn(c.id)}>{t('extensoes.entrar')}</Button> : null}
                          <RemoveButton title={c.name} onClick={() => setRemoving({ id: c.id, name: c.name })} />
                        </>
                      )}
                    />
                  ))}
                </ItemGrid>
              </MarketSection>
            ) : null}
            {/* D166: com recibo e remoção pelo digest (views/fork/catalogo-skills). */}
            {installedSkills.length ? <SkillsInstaladas /> : null}
          </div>
        </div>
      </div>
      <Dialog
        open={removing !== null}
        title={t('extensoes.removerTitulo', { nome: isolado(removing?.name ?? '') })}
        message={removing ? t('extensoes.removerConectorMensagem') : null}
        actions={removing ? [{ title: t('extensoes.remover'), destructive: true, action: () => void remove(removing) }] : []}
        cancel={t('extensoes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setRemoving(null)}
      />
      </div>
    </SheetPage>
  )
}

/** A segunda linha de um MCP: a orientação traduzida, ou o comando/URL/descrição salvos como vieram. */
function textoDoDetalhe(d: Detalhe, t: ReturnType<typeof useT>) {
  return 'chave' in d ? t(d.chave) : 'codigo' in d ? d.codigo : d.dado
}

function RemoveButton({ title, onClick }: { title: string; onClick: () => void }) {
  const t = useT()
  return (
    <button
      title={t('extensoes.remover')}
      aria-label={t('extensoes.removerNome', { nome: isolado(title) })}
      className="market-remove"
      onClick={(e) => {
        e.stopPropagation()
        onClick()
      }}
    >
      <Icon name="trash" size={13} />
    </button>
  )
}
