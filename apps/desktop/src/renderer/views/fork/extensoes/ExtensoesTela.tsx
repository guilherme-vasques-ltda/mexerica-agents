// Auditoria D249 (E3): a tela de Extensões, no lugar da barra lateral e da conversa, com a camada, a placa e o
// cabeçalho de Ajustes e Rotinas, e o corpo de Ajustes: a lista das páginas num cartão e a página no outro.
// O conteúdo de cada página é o MarketplaceView de sempre, embutido (sem o próprio título, abas e Fechar).
import { Fragment, useEffect, useRef, useState } from 'react'
import type { PaginaLoja } from './tela'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { IconSlot } from '../../../components/Stack'
import { useEscape } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { StoreContext, useApp } from '../../../store/context'
import { MarketplaceView } from '../../marketplace/MarketplaceView'
import { extensoesScreen } from './lugar-atual'
import { ehPaginaLoja, PAGINAS_EXTENSOES, paginaDisponivel } from './tela'
import { CredentialsView } from '../../marketplace/CredentialsView'
import { PaginaPlugins } from './PaginaPlugins'
import { settingsScreen } from '../ajustes/tela'
import './extensoes.css'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import '../../settings/settings.css'
import '../ajustes/ajustes-tela.css'

export { extensoesScreen }

/** A frase sob o título de cada página. A de Skills é a que a página de Skills já usava. */
const SOBRE = {
  connectors: 'extensoes.conectores.sobre',
  skills: 'skills.descricaoPagina',
  plugins: 'extensoes.plugins.sobre',
  credentials: 'extensoes.cred.sobre',
} as const

export function ExtensoesTela() {
  const tela = useModel(extensoesScreen)
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const back = useRef<HTMLButtonElement>(null)
  const ultimaLoja = useRef<PaginaLoja | null>(null)
  // Cada clique em "Adicionar conector personalizado" pede à loja que abra a folha (o fluxo, com o aviso de não verificado).
  const [pedidoAdicionar, setPedidoAdicionar] = useState(0)
  useEffect(() => back.current?.focus({ preventScroll: true }), [])
  // Uma folha aberta dentro das Extensões (Adicionar MCP, um diálogo) entra depois na pilha e fecha antes.
  useEscape(tela.open, () => tela.close(), back)
  const store = tela.computer ? app.store(tela.computer) : null
  if (!tela.page) ultimaLoja.current = null
  if (!tela.page || !store) return null
  const atual = PAGINAS_EXTENSOES.find((p) => p.id === tela.page)!
  // Velocidade: Aplicativos, Conectores e Skills são UMA instância do MarketplaceView, que fica montada (escondida em
  // Plugins e Credenciais) enquanto a tela está aberta. Trocar de página não remonta nem busca o catálogo de novo.
  if (ehPaginaLoja(atual.id)) ultimaLoja.current = atual.id
  const loja = ultimaLoja.current
  const online = [...app.stores.values()].filter((s) => s.connection.kind === 'online')
  return (
    // D454 (fatia 8a): a tela inteira é traduzida; só a página de Skills (catálogo, fatia 8b) ainda é legado e
    // declara o limite dela (MarketplaceView).
    <div className="rc-page aj-page idioma-superficie" {...superficieIdioma(idioma)} role="region" aria-label={t('extensoes.titulo')}>
      <header className="rc-header drag">
        <button ref={back} className="rc-back no-drag" aria-label={t('extensoes.voltar')} title={t('extensoes.voltarEsc')} onClick={() => tela.close()}>
          <Icon name="arrow.left" size={14} weight="medium" />
        </button>
        <h1 className="rc-title">{t('extensoes.titulo')}</h1>
      </header>
      <div className="settings">
        <nav className="settings-sidebar" aria-label={t('extensoes.paginas')}>
          {PAGINAS_EXTENSOES.filter((p) => paginaDisponivel(p.id)).map((p) => (
            <Fragment key={p.id}>
            {p.id === 'credentials' ? <div className="ext-lateral-divisor" role="presentation" /> : null}
            <button className={`settings-item ${p.id === tela.page ? 'selected' : ''}`} aria-current={p.id === tela.page ? 'page' : undefined} onClick={() => tela.setPage(p.id)}>
              <IconSlot width={18} tone="secondary"><Icon name={p.icon} size={13} /></IconSlot>
              <Text truncate grow>{t(p.chave)}</Text>
            </button>
            </Fragment>
          ))}
        </nav>
        <section className="settings-main" aria-label={t(atual.chave)}>
          {/* As cinco páginas têm o mesmo cabeçalho: título, uma frase e a linha. Só um plugin aberto troca pelo dele, com Voltar. */}
          {atual.id === 'plugins' && tela.plugin ? null : (
            <div className="settings-head">
              {atual.id === 'connectors' ? (
                <div className="ext-titulo-linha">
                  <h2 className="settings-title">{t(atual.chave)}</h2>
                  <Button kind="secondary" onClick={() => setPedidoAdicionar((n) => n + 1)}><Icon name="plus" size={12} />{t('mercado.adicionarPersonalizado')}</Button>
                </div>
              ) : <h2 className="settings-title">{t(atual.chave)}</h2>}
              <p className="ext-subtitulo">{t(SOBRE[atual.id])}</p>
            </div>
          )}
          {loja ? (
            <div key={`${tela.computer}:loja`} className="settings-page" hidden={!ehPaginaLoja(atual.id)}>
              <StoreContext.Provider value={store}>
                <MarketplaceView key={tela.computer} embedded={loja} ativa={ehPaginaLoja(atual.id)} pedidoAdicionar={pedidoAdicionar} computers={online.map((s) => ({ id: s.computer.id, name: s.hostName }))}
                  knownComputers={app.computers.length} computer={tela.computer!} onComputer={(id) => tela.setComputer(id)} />
              </StoreContext.Provider>
            </div>
          ) : null}
          {ehPaginaLoja(atual.id) ? null : (
            <div key={`${tela.computer}:${atual.id}`} className="settings-page ext-pagina-propria">
              <StoreContext.Provider value={store}>
                {atual.id === 'plugins'
                  ? <PaginaPlugins store={store} aberto={tela.plugin} onAbrir={(id) => tela.setPlugin(id)} openVoice={() => settingsScreen.show('voice')} />
                  : <CredentialsView embedded />}
              </StoreContext.Provider>
            </div>
          )}
        </section>
      </div>
    </div>
  )
}
