// Google direto (MEX-1447): o fluxo de conectar os serviços do Google com o app OAuth da própria pessoa. Sem React nem
// DOM (só `.ts` nos imports, para os testes do Node rodarem direto). Três passos: o app (guia + ID e chave), a escolha
// dos serviços e o login, um serviço por vez. A chave secreta só passa por `salvarApp` e nunca é guardada aqui.
import { Observable } from '../../../lib/observable.ts'
import { mercadoApi } from './api.ts'
import type { FluxoConectar, Passo } from './fluxo.ts'
import { jeitoDoGoogle } from './regras.ts'
import type { AppGoogle, Servico, TipoConexao } from './tipos.ts'

/** O passo de agora (lido de novo: o TypeScript não vê que `confirmar`/`entrar` o mudam). */
const passoDe = (f: FluxoConectar): Passo => f.passo
const mensagem = (e: unknown) => (e instanceof Error ? e.message : String(e))

export type PassoGoogle = 'guia' | 'servicos' | 'entrar' | 'pronto'
export type EstadoDoLogin = 'espera' | 'entrando' | 'conferindo' | 'pronto' | 'erro'

export interface DependenciasGoogle {
  cliente(): Promise<import('../../../client/host-client').HostClient>
  /** Relê catálogo e contas depois de cada escrita. */
  reler(): Promise<void>
  /** O fluxo de conectar UMA conta pela ficha (o mesmo de qualquer serviço); o do Google só o conduz em fila. */
  criarFluxo(servico: Servico, tipo: TipoConexao): FluxoConectar
}

export class FluxoGoogle extends Observable {
  passo: PassoGoogle
  app: AppGoogle | null
  ocupado = false
  /** A recusa do Hub ao salvar o app (texto bruto; a tela o traduz pelo código estável). */
  erroDoApp: string | null = null
  /** Os serviços marcados, na ordem do catálogo. */
  private marcados: Set<string>
  private readonly fluxos = new Map<string, FluxoConectar>()
  readonly servicos: Servico[]
  private readonly deps: DependenciasGoogle

  constructor(servicos: readonly Servico[], app: AppGoogle | null, deps: DependenciasGoogle) {
    super()
    this.deps = deps
    this.app = app
    this.servicos = servicos.filter((s) => jeitoDoGoogle(s) !== null)
    for (const s of this.servicos) {
      const tipo = jeitoDoGoogle(s)
      if (tipo) this.fluxos.set(s.id, deps.criarFluxo(s, tipo))
    }
    // Cada serviço já vem marcado e só lendo (o padrão seguro da ficha); a pessoa desmarca o que não quer.
    this.marcados = new Set(this.servicos.filter((s) => s.conectavel).map((s) => s.id))
    this.passo = app?.configurado ? 'servicos' : 'guia'
  }

  get configurado(): boolean { return this.app?.configurado === true }
  fluxo(id: string): FluxoConectar | null { return this.fluxos.get(id) ?? null }
  marcado(id: string): boolean { return this.marcados.has(id) }
  get totalMarcados(): number { return this.marcados.size }

  // ───────── 1. o app ─────────

  /** Guarda o ID e a chave no cofre do Hub. A resposta traz só o ID; os dois valores saem da memória do chamador. */
  async salvarApp(clientId: string, clientSecret: string): Promise<boolean> {
    if (this.ocupado || clientId.trim() === '' || clientSecret.trim() === '') return false
    this.ocupado = true; this.erroDoApp = null; this.changed()
    try {
      this.app = await mercadoApi.salvarAppGoogle(await this.deps.cliente(), clientId.trim(), clientSecret.trim())
      await this.deps.reler()
      this.passo = 'servicos'
      this.ocupado = false; this.changed()
      return true
    } catch (e) {
      this.erroDoApp = mensagem(e)
      this.ocupado = false; this.changed()
      return false
    }
  }

  /** Volta ao guia para trocar o app (o Hub encerra o login das contas Google do Space ao salvar o novo). */
  trocarApp() { this.erroDoApp = null; this.passo = 'guia'; this.changed() }
  voltarAosServicos() { this.erroDoApp = null; this.passo = this.configurado ? 'servicos' : 'guia'; this.changed() }

  // ───────── 2. os serviços ─────────

  alternarServico(id: string) {
    if (!this.fluxos.has(id) || this.passo !== 'servicos') return
    if (this.marcados.has(id)) this.marcados.delete(id)
    else this.marcados.add(id)
    this.changed()
  }

  alternarGrupo(servico: string, grupo: string) {
    this.fluxos.get(servico)?.alternar(grupo)
    this.changed()
  }

  nomear(servico: string, campo: 'nova' | 'primeira', valor: string) {
    this.fluxos.get(servico)?.nomear(campo, valor)
    this.changed()
  }

  private get daFila(): FluxoConectar[] {
    return this.servicos.filter((s) => this.marcados.has(s.id)).flatMap((s) => this.fluxos.get(s.id) ?? [])
  }

  get podeContinuar(): boolean {
    return this.passo === 'servicos' && !this.ocupado && this.configurado && this.daFila.length > 0 && this.daFila.every((f) => f.podeContinuar)
  }

  // ───────── 3. o login, um serviço por vez ─────────

  estadoDe(id: string): EstadoDoLogin {
    const f = this.fluxos.get(id)
    if (!f) return 'espera'
    if (f.passo === 'pronto') return 'pronto'
    if (f.passo === 'conferindo') return 'conferindo'
    if (f.passo === 'configurar') return f.erro ? 'erro' : f.ocupado ? 'entrando' : 'espera'
    return f.falhaDeEntrada !== null ? 'erro' : 'entrando'
  }

  /** O texto bruto da falha de um serviço (para "Ver detalhes" e para achar o código estável do Hub). */
  falhaDe(id: string): string | null {
    const f = this.fluxos.get(id)
    return f ? (f.erro?.texto ?? f.falhaDeEntrada) : null
  }

  async continuar(): Promise<void> {
    if (!this.podeContinuar) return
    this.passo = 'entrar'; this.changed()
    await this.rodar()
  }

  async tentarDeNovo(): Promise<void> {
    if (this.passo !== 'entrar' || this.ocupado) return
    await this.rodar()
  }

  /** Passa pela fila: cada serviço conecta e entra no Google; para na primeira falha (a tela mostra e oferece repetir). */
  private async rodar(): Promise<void> {
    this.ocupado = true; this.changed()
    try {
      for (const f of this.daFila) {
        if (f.passo === 'pronto') continue
        // Conta já criada que falhou no login: só repete o login. Sem conta ainda: cria (o Hub confere nome e ficha).
        if (f.conta) await f.entrar()
        else await f.confirmar()
        this.changed()
        if (passoDe(f) !== 'pronto') return
      }
      this.passo = 'pronto'
    } finally {
      this.ocupado = false; this.changed()
    }
  }

  /** Desistir antes do fim: as contas criadas pela metade saem (só o registro delas); as prontas ficam. */
  async cancelar(): Promise<void> {
    await Promise.all([...this.fluxos.values()].map((f) => f.cancelar()))
  }
}
