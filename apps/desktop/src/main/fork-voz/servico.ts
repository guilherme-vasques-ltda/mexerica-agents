import { randomUUID } from 'node:crypto'
import { catalogoXai, conferirXai, credencialXai } from './xai.ts'
import { confereSessaoXai, idSessaoXai, pedidoXai, type PedidoXai, type ResultadoCatalogoXai, type ResultadoCredencialXai, type ResultadoSessaoXai } from '../../shared/fork-voz-xai.ts'
import type { Resultado } from './provedor.ts'
import { ehProvedorAparelho, formatoDeChave, geminiComFerramentasAssincronas, nomeValido, VOZ_PROVEDORES_APARELHO, type CategoriaChave, type EstadoChaves, type ResultadoChave, type ResultadoCredencial, type VozProvedorAparelho } from '../../shared/fork-voz.ts'
import { CofreVoz, FalhaCofre } from './cofre.ts'
import { conferirChave, cunharCredencial, type Buscar } from './provedor.ts'

// voz-04 (plano §2): as intenções da tela Voz sobre a chave deste aparelho e os canais IPC que as
// recebem. Cada canal confere quem pede (só a janela de chat), o número e o formato dos argumentos e
// responde só estado, categoria ou a credencial curta. Nenhum canal lê a chave, nenhum erro sai com
// texto do provedor ou do sistema, e nada daqui escreve em log.

const categoriaDoCofre = (e: unknown): CategoriaChave => (e instanceof FalhaCofre ? e.categoria : 'cofre')

export class ServicoVoz {
  private readonly negociacoes = new Map<string, { pedido: PedidoXai; geracao: number; prazo: number; sessao: string | null }>()
  private readonly cofre: CofreVoz
  private readonly buscar: Buscar
  private readonly avisar: () => void

  constructor(cofre: CofreVoz, buscar: Buscar, avisar: () => void = () => {}) {
    this.cofre = cofre
    this.buscar = buscar
    this.avisar = avisar
  }

  estado(): Promise<EstadoChaves> { return this.cofre.estado() }

  private conferir(p: VozProvedorAparelho, chave: string) {
    return p === 'xai' ? conferirXai(chave, this.buscar) : conferirChave(p, chave, this.buscar)
  }

  private async resultado(ok: boolean, categoria: CategoriaChave | null): Promise<ResultadoChave> {
    return { ok, categoria, estado: await this.cofre.estado() }
  }

  /** Confere com o provedor antes de cifrar: chave recusada, sem rede ou sem cofre aceito não grava nada. */
  async guardar(p: VozProvedorAparelho, chave: string): Promise<ResultadoChave> {
    if (p === 'xai') this.negociacoes.clear()
    const geracao = this.cofre.novaGeracao(p)
    if (!this.cofre.seguro()) return this.resultado(false, 'sem-cofre')
    const recusa = await this.conferir(p, chave)
    try {
      await this.cofre.naGeracao(p, geracao)
      if (recusa) return this.resultado(false, recusa)
      await this.cofre.guardar(p, chave, geracao)
    } catch (e) {
      this.avisar()
      return this.resultado(false, categoriaDoCofre(e))
    }
    this.avisar()
    return this.resultado(true, null)
  }

  async limpar(p: VozProvedorAparelho): Promise<ResultadoChave> {
    if (p === 'xai') this.negociacoes.clear()
    let categoria: CategoriaChave | null = null
    try { await this.cofre.limpar(p) } catch (e) { categoria = categoriaDoCofre(e) }
    this.avisar()
    return this.resultado(categoria === null, categoria)
  }

  /** "Testar", "Tentar de novo" e "Usar a chave deste Mac": abre do zero e confere com o provedor, sem sessão de áudio. Deu certo: a chave daqui volta a valer. */
  async testar(p: VozProvedorAparelho): Promise<ResultadoChave> {
    const geracao = this.cofre.geracao(p)
    this.cofre.esquecer(p)
    let chave: string | null
    try { chave = await this.cofre.tentarAbrir(p) } catch (e) {
      this.avisar()
      return this.resultado(false, categoriaDoCofre(e))
    }
    if (chave === null) { this.avisar(); return this.resultado(false, 'sem-chave') }
    const recusa = await this.conferir(p, chave)
    try {
      await this.cofre.naGeracao(p, geracao, () => {
        if (recusa === 'chave') this.cofre.marcarRecusada(p)
        else if (recusa === null) { this.cofre.marcarAceita(p); this.cofre.voltarAoAparelho(p) }
      })
    } catch (e) {
      this.avisar()
      return this.resultado(false, categoriaDoCofre(e))
    }
    this.avisar()
    return this.resultado(recusa === null, recusa)
  }

  async usarReserva(p: VozProvedorAparelho): Promise<ResultadoChave> {
    if (p === 'xai') return this.resultado(false, 'invalida')
    // A situação vem de abrir de novo: a escolha só vale com a chave travada, recusada ou incerta agora.
    await this.cofre.estado()
    const ok = this.cofre.escolherReserva(p)
    if (ok) this.avisar()
    return this.resultado(ok, ok ? null : 'invalida')
  }

  /** A credencial curta do Tempo real, com a chave deste aparelho. Sem chave ou cofre fechado: categoria, nunca o Hub. */
  async credencial(p: VozProvedorAparelho, model: string, voice: string): Promise<ResultadoCredencial> {
    if (p === 'xai') return { ok: false, categoria: 'invalida' }
    const geracao = this.cofre.geracao(p)
    if (p === 'gemini' && !geminiComFerramentasAssincronas(model)) return { ok: false, categoria: 'invalida' }
    let chave: string | null
    try { chave = await this.cofre.abrir(p) } catch (e) {
      this.avisar()
      return { ok: false, categoria: categoriaDoCofre(e) }
    }
    if (chave === null) return { ok: false, categoria: 'sem-chave' }
    const r = await cunharCredencial(p, chave, model, voice, this.buscar)
    try {
      await this.cofre.naGeracao(p, geracao, () => {
        if (r.ok) this.cofre.marcarAceita(p)
        else if (r.categoria === 'chave') { this.cofre.marcarRecusada(p); this.avisar() }
      })
    } catch (e) {
      this.avisar()
      return { ok: false, categoria: categoriaDoCofre(e) }
    }
    return r.ok ? { ok: true, credential: r.valor } : { ok: false, categoria: r.categoria }
  }

  /** Mesmas guardas de geração do voz-04; a chave longa só existe no callback do main. */
  private async comChave<T>(p: VozProvedorAparelho, fazer: (chave: string, validar: () => Promise<void>) => Promise<Resultado<T>>): Promise<Resultado<T>> {
    const geracao = this.cofre.geracao(p)
    try {
      const chave = await this.cofre.abrir(p)
      if (chave === null) return { ok: false, categoria: 'sem-chave' }
      const validar = () => this.cofre.naGeracao(p, geracao)
      await validar()
      const r = await fazer(chave, validar)
      await this.cofre.naGeracao(p, geracao, () => {
        if (r.ok) this.cofre.marcarAceita(p)
        else if (r.categoria === 'chave') { this.cofre.marcarRecusada(p); this.avisar() }
      })
      return r
    } catch (e) { return { ok: false, categoria: e instanceof FalhaCofre ? e.categoria : 'provedor' } }
  }

  async catalogoXai(): Promise<ResultadoCatalogoXai> {
    const r = await this.comChave('xai', async chave => {
      const c = await catalogoXai(chave, this.buscar)
      return c.ok ? { ok: true, valor: c.vozes } : c
    })
    return r.ok ? { ok: true, vozes: r.valor } : r
  }

  async credencialXai(pedido: PedidoXai): Promise<ResultadoCredencialXai> {
    if (!pedidoXai(pedido)) return { ok: false, categoria: 'invalida' }
    for (const [id, n] of this.negociacoes) if (n.prazo <= Date.now()) this.negociacoes.delete(id)
    if (this.negociacoes.size >= 8) return { ok: false, categoria: 'limite' }
    // Snapshot antes de qualquer await; a sessão não é escolhida pelo pedido de conferência.
    const copia = { ...pedido }, geracao = this.cofre.geracao('xai')
    const r = await this.comChave('xai', (chave, validar) => credencialXai(chave, copia, this.buscar, Date.now, validar))
    if (!r.ok) return r
    const negociacao = randomUUID()
    let cheia = false
    try {
      await this.cofre.naGeracao('xai', geracao, () => {
        for (const [id, n] of this.negociacoes) if (n.prazo <= Date.now()) this.negociacoes.delete(id)
        cheia = this.negociacoes.size >= 8
        if (!cheia) this.negociacoes.set(negociacao, { pedido: copia, geracao, prazo: Date.now() + 15_000, sessao: null })
      })
    } catch (e) { return { ok: false, categoria: categoriaDoCofre(e) } }
    return cheia ? { ok: false, categoria: 'limite' } : { ok: true, credential: r.valor, negociacao }
  }

  async conferirSessaoXai(id: string, sessao: unknown): Promise<ResultadoSessaoXai> {
    const n = this.negociacoes.get(id)
    if (!n) return { ok: false, categoria: 'cancelado' }
    try {
      let ok = false
      await this.cofre.naGeracao('xai', n.geracao, () => {
        const s = sessao as { id?: unknown } | null
        if (this.negociacoes.get(id) !== n || n.prazo <= Date.now() || !s || !idSessaoXai(s.id) ||
          (n.sessao !== null && n.sessao !== s.id) || JSON.stringify(sessao).length > 64_000 || !confereSessaoXai(sessao, n.pedido)) return
        if (n.sessao === null) { n.sessao = s.id; n.prazo = Date.now() + 1_800_000 }
        ok = true
      })
      if (!ok) this.negociacoes.delete(id)
      return ok ? { ok: true } : { ok: false, categoria: 'provedor' }
    } catch (e) {
      this.negociacoes.delete(id)
      return { ok: false, categoria: e instanceof FalhaCofre ? e.categoria : 'provedor' }
    }
  }

  async fecharSessaoXai(id: string): Promise<ResultadoSessaoXai> {
    this.negociacoes.delete(id)
    return { ok: true }
  }

}

/** O mínimo de `ipcMain` que os canais usam (injetado nos testes). */
export interface CanaisIPC {
  handle(canal: string, fn: (event: unknown, ...args: unknown[]) => unknown): void
}

const ESTADO_VAZIO: EstadoChaves = {
  cofre: 'sem-cofre',
  chaves: Object.fromEntries(VOZ_PROVEDORES_APARELHO.map((p) => [p, { situacao: 'ausente', reserva: false }])) as EstadoChaves['chaves'],
}
const recusa = (categoria: CategoriaChave): ResultadoChave => ({ ok: false, categoria, estado: ESTADO_VAZIO })

/** Os canais fixos `voz:*`. `permitido`: o remetente é o quadro principal da janela de chat. */
export function instalarCanaisVoz(ipc: CanaisIPC, servico: ServicoVoz, permitido: (event: unknown) => boolean) {
  const canal = <T>(nome: string, aridade: number, negado: T, fazer: (args: unknown[]) => Promise<T> | null) => {
    ipc.handle(`voz:${nome}`, async (event, ...args) => {
      if (!permitido(event)) return nome === 'estado' ? ESTADO_VAZIO : negado
      if (args.length !== aridade) return nome === 'estado' ? ESTADO_VAZIO : invalido(nome)
      try {
        const trabalho = fazer(args)
        return trabalho ? await trabalho : invalido(nome)
      } catch {
        // Nada do erro sai daqui nem vai para o log: só a categoria genérica.
        return nome === 'credencial' || nome.startsWith('xai:') ? { ok: false, categoria: 'provedor' } : nome === 'estado' ? ESTADO_VAZIO : recusa('provedor')
      }
    })
  }
  const invalido = (nome: string) => (nome === 'credencial' || nome.startsWith('xai:') ? { ok: false, categoria: 'invalida' } : recusa('invalida'))
  const negado = recusa('origem')

  const idXai = (v: unknown): v is string => typeof v === 'string' && /^[a-f0-9-]{36}$/.test(v)
  canal<ResultadoCatalogoXai>('xai:catalogo', 0, { ok: false, categoria: 'origem' }, () => servico.catalogoXai())
  canal<ResultadoCredencialXai>('xai:credencial', 1, { ok: false, categoria: 'origem' }, ([p]) => pedidoXai(p) ? servico.credencialXai(p) : null)
  canal<ResultadoSessaoXai>('xai:conferir-sessao', 2, { ok: false, categoria: 'origem' }, ([id, s]) => idXai(id) ? servico.conferirSessaoXai(id, s) : null)
  canal<ResultadoSessaoXai>('xai:fechar-sessao', 1, { ok: false, categoria: 'origem' }, ([id]) => idXai(id) ? servico.fecharSessaoXai(id) : null)
  canal('estado', 0, ESTADO_VAZIO, () => servico.estado())
  canal('guardar', 2, negado, ([p, chave]) => {
    if (!ehProvedorAparelho(p) || typeof chave !== 'string') return null
    const limpa = chave.trim()
    return formatoDeChave(limpa) ? servico.guardar(p, limpa) : null
  })
  canal('limpar', 1, negado, ([p]) => (ehProvedorAparelho(p) ? servico.limpar(p) : null))
  canal('testar', 1, negado, ([p]) => (ehProvedorAparelho(p) ? servico.testar(p) : null))
  canal('usarReserva', 1, negado, ([p]) => (ehProvedorAparelho(p) ? servico.usarReserva(p) : null))
  canal<ResultadoCredencial>('credencial', 3, { ok: false, categoria: 'origem' }, ([p, model, voice]) =>
    ehProvedorAparelho(p) && nomeValido(model) && nomeValido(voice) ? servico.credencial(p, model, voice) : null)
}
