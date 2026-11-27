// Auditoria D249 (E3; decisão do coordenador D229): Extensões é uma tela, como Ajustes, não mais uma folha.
// Modelo sem DOM nem React: se a tela está aberta, em que página e em que computador. Cada atalho abre a sua
// página: Conectores (faixa e engrenagem) abre o catálogo e as contas, Skills abre Skills.
import { Observable } from '../../../lib/observable.ts'
import { isOpenDesktop } from '../../../../shared/fork-capacidades.ts'

// Pedido do dono em 17/11/2026: Aplicativos e Conectores eram a mesma coisa; sobrou só Conectores.
// Revisão do app (Entrega 2, pedido do dono em 10/11/2026): Plugins sai de Ajustes e vira a quarta página;
// Credenciais deixa de ser um botão no cabeçalho comum e vira a quinta. Cada página mostra só o que é dela.
export type PaginaExtensoes = 'connectors' | 'skills' | 'plugins' | 'credentials'
/** As páginas que a loja (MarketplaceView) desenha; Plugins e Credenciais têm tela própria. */
export type PaginaLoja = 'connectors' | 'skills'
export const ehPaginaLoja = (p: PaginaExtensoes): p is PaginaLoja => p === 'connectors' || p === 'skills'
export const extensoesSomenteCatalogo = () => isOpenDesktop(globalThis.window?.codync)
export const paginaDisponivel = (page: PaginaExtensoes) => page === 'connectors' || page === 'skills' || page === 'credentials' || !extensoesSomenteCatalogo()

/** D454 (fatia 8a): cada página leva só a chave do catálogo; a tela traduz no idioma escolhido. */
export const PAGINAS_EXTENSOES: { id: PaginaExtensoes; chave: `extensoes.pagina.${PaginaExtensoes}`; icon: string }[] = [
  { id: 'connectors', chave: 'extensoes.pagina.connectors', icon: 'puzzlepiece.extension' },
  { id: 'skills', chave: 'extensoes.pagina.skills', icon: 'lightbulb' },
  { id: 'plugins', chave: 'extensoes.pagina.plugins', icon: 'shippingbox' },
  { id: 'credentials', chave: 'extensoes.pagina.credentials', icon: 'key' },
]

export class TelaExtensoes extends Observable {
  /** A página aberta; `null` com a tela fechada. */
  page: PaginaExtensoes | null = null
  /** O computador cujo Hub a tela mostra (o da conversa, senão o local, senão o primeiro online). */
  computer: string | null = null
  /** Em Plugins: o plugin aberto; `null` mostra a lista. Um plugin só abre com um clique na linha dele. */
  plugin: string | null = null
  /** Abre: fecha antes o lugar atual (Rotinas, Times, Ajustes). */
  aoAbrir: () => void = () => {}
  /** Devolve o foco ao fechar pelo Voltar ou pelo Esc. */
  aoFechar: () => void = () => {}

  get open() { return this.page !== null && this.computer !== null }

  /** `plugin`: abre Plugins já dentro dele (o atalho do WhatsApp nas Configurações do bot). */
  show(page: PaginaExtensoes, computer: string | null, plugin: string | null = null) {
    if (!computer || !paginaDisponivel(page)) return
    this.aoAbrir()
    this.page = page
    this.computer = computer
    this.plugin = page === 'plugins' ? plugin : null
    this.changed()
  }

  /** Trocar de página, ou clicar em Plugins de novo, volta à lista fechada. */
  setPage(page: PaginaExtensoes) {
    if (!this.open || !paginaDisponivel(page) || (this.page === page && this.plugin === null)) return
    this.page = page
    this.plugin = null
    this.changed()
  }

  /** Abre um plugin (com um id) ou volta à lista (`null`); só vale na página Plugins. */
  setPlugin(plugin: string | null) {
    if (this.page !== 'plugins' || this.plugin === plugin) return
    this.plugin = plugin
    this.changed()
  }

  setComputer(computer: string) {
    if (!this.open || this.computer === computer) return
    this.computer = computer
    this.changed()
  }

  /** `restoreFocus`: Voltar e Esc devolvem o foco; ir a outro lugar não. */
  close(restoreFocus = true) {
    if (!this.open) return
    this.page = null
    this.computer = null
    this.plugin = null
    this.changed()
    if (restoreFocus) this.aoFechar()
  }
}

export const extensoesScreen = new TelaExtensoes()
