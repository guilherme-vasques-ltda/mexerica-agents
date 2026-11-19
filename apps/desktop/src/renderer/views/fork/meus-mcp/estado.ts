import { connectorConfiguration, needsSignIn, type InstalledConnector } from '../../marketplace/market-models.ts'

// D175 (docs/features/marketplace.md, "Na tela"): quem aparece em Meus MCP, os selos honestos de cada
// configuração e a leitura presa à origem que respondeu (Hub, link vivo e geração, a tupla do D166).
// Sem chamada própria: quem lê passa a função que busca.

/** Busca local na lista do usuário: nome, descrição, comando e URL salvos. */
export function filtrarLocal(itens: readonly InstalledConnector[], busca: string): InstalledConnector[] {
  const q = busca.trim().toLocaleLowerCase('pt-BR')
  if (!q) return [...itens]
  return itens.filter((c) => [c.name, c.description, c.command, c.url].some((t) => (t ?? '').toLocaleLowerCase('pt-BR').includes(q)))
}

// D454 (fatia 8a, R1 do #577): o estado fica como chave do catálogo e é traduzido ao desenhar; comando, URL e
// descrição salvos são dados e aparecem como vieram.
export interface Selo {
  id: 'manual' | 'entrar'
  chave: 'extensoes.mcp.selo.manual' | 'extensoes.mcp.selo.entrar'
  icone: string
}

/** O que a linha afirma: só alvo ausente e login pendente. Nada diz "pronto" ou "disponível". */
export function selosDe(c: InstalledConnector): Selo[] {
  const selos: Selo[] = []
  if (connectorConfiguration(c) === 'manualRequired') selos.push({ id: 'manual', chave: 'extensoes.mcp.selo.manual', icone: 'exclamationmark.triangle' })
  if (needsSignIn(c)) selos.push({ id: 'entrar', chave: 'extensoes.mcp.selo.entrar', icone: 'key.fill' })
  return selos
}

/** A segunda linha: o alvo salvo (dado), ou a orientação manual (chave do catálogo) quando ele falta. */
/** A segunda linha de um MCP: orientação do catálogo, código salvo (comando ou URL, LTR isolado) ou a descrição (R2-01 do #577). */
export type Detalhe = { chave: 'extensoes.mcp.detalhe.manual' } | { codigo: string } | { dado: string }

export function detalheDe(c: InstalledConnector): Detalhe {
  if (connectorConfiguration(c) === 'manualRequired') return { chave: 'extensoes.mcp.detalhe.manual' }
  return codigoOuDescricao(c)
}

/** Comando ou URL salvos são código; só sem os dois vem a descrição, que é texto de quem cadastrou. */
export function codigoOuDescricao(c: Pick<InstalledConnector, 'command' | 'url' | 'description'>): { codigo: string } | { dado: string } {
  const codigo = c.command ?? c.url
  return codigo != null ? { codigo } : { dado: c.description }
}

/**
 * Uma escrita sem resposta clara (o link caiu, timeout: `HostError` com status 0, ou um erro sem status)
 * pode ter acontecido no Hub. Não é sucesso nem "nada feito": relê-se a lista antes de outro gesto.
 */
export function resultadoIncerto(e: unknown): boolean {
  if (typeof e !== 'object' || e === null) return true
  const status = (e as { status?: unknown }).status
  return typeof status !== 'number' || status === 0
}

/** De onde uma leitura ou um gesto partiu: o Hub, o link vivo e a geração dele. */
export type Origem<C> = Readonly<{ hub: string; client: C; geracao: number }>

export type FonteConectores<C> = {
  readonly hello: { hostId: string } | null
  readonly connectedClient: C | null
  readonly principalEpoch: number
}

export function origemDe<C>(fonte: FonteConectores<C>): Origem<C> | null {
  const hub = fonte.hello?.hostId
  const client = fonte.connectedClient
  return hub && client ? { hub, client, geracao: fonte.principalEpoch } : null
}

/** A conexão viva ainda é exatamente a da origem: mesmo Hub, mesmo link e mesma geração. */
export function mesmaOrigem<C>(fonte: FonteConectores<C>, origem: Origem<C> | null): boolean {
  return origem !== null && fonte.hello?.hostId === origem.hub && fonte.connectedClient === origem.client && fonte.principalEpoch === origem.geracao
}

export type EstadoLista = 'semConexao' | 'lendo' | 'erro' | 'lida'

/**
 * A lista de conectores do Hub, presa à origem que respondeu. Resposta de outra origem ou de um pedido
 * já superado não é publicada; troca de Hub, reconexão ou offline escondem a lista anterior; erro de
 * leitura é erro, nunca lista vazia.
 */
export class LeituraConectores<C> {
  private lida: { origem: Origem<C>; itens: InstalledConnector[]; ordem: number } | null = null
  private falha: { origem: Origem<C>; mensagem: string; ordem: number } | null = null
  private pedido = 0

  /**
   * D175 F2: a marca de agora, o número do último pedido. Só conta depois dela uma leitura **pedida** depois
   * (`ordem` é o número do pedido que publicou): uma que já estava em andamento pode ter sido respondida antes.
   */
  marca(): number {
    return this.pedido
  }

  /**
   * Reconciliação positiva: uma lista da origem atual, pedida depois da marca, foi lida com sucesso e nenhuma
   * falha veio depois dela. Um erro publicado, por si, nunca conta.
   */
  confirmadaDepois(fonte: FonteConectores<C>, marca: number): boolean {
    return this.lida !== null && this.lida.ordem > marca && mesmaOrigem(fonte, this.lida.origem)
      && (this.falha === null || this.falha.ordem < this.lida.ordem)
  }

  /** O erro de uma leitura da origem atual pedida depois da marca, se foi a última. */
  erroDepois(fonte: FonteConectores<C>, marca: number): string | null {
    return this.falha && this.falha.ordem > marca && mesmaOrigem(fonte, this.falha.origem) ? this.falha.mensagem : null
  }

  /** Os conectores da origem de agora; os de outra origem nunca aparecem. */
  itens(fonte: FonteConectores<C>): InstalledConnector[] {
    return this.lida && mesmaOrigem(fonte, this.lida.origem) ? this.lida.itens : []
  }

  erro(fonte: FonteConectores<C>): string | null {
    return this.falha && mesmaOrigem(fonte, this.falha.origem) ? this.falha.mensagem : null
  }

  estado(fonte: FonteConectores<C>): EstadoLista {
    if (!origemDe(fonte)) return 'semConexao'
    if (this.erro(fonte) !== null) return 'erro'
    return this.lida && mesmaOrigem(fonte, this.lida.origem) ? 'lida' : 'lendo'
  }

  /**
   * Relê pela origem de agora; `true` quando publicou a lista **ou o erro** dessa origem. Para saber se houve
   * leitura positiva depois de um gesto, use `confirmadaDepois` com a marca tomada quando ele terminou.
   */
  async ler(fonte: FonteConectores<C>, buscar: (client: C) => Promise<InstalledConnector[]>): Promise<boolean> {
    const origem = origemDe(fonte)
    if (!origem) return false
    const meu = ++this.pedido
    const vale = () => meu === this.pedido && mesmaOrigem(fonte, origem)
    try {
      const itens = await buscar(origem.client)
      if (!vale()) return false
      this.lida = { origem, itens, ordem: meu }
      this.falha = null
      return true
    } catch (e) {
      if (!vale()) return false
      this.falha = { origem, mensagem: e instanceof Error ? e.message : String(e), ordem: meu }
      return true
    }
  }
}
