// Marketplace (fatia 1b): o fluxo de conectar uma conta pela ficha. Sem React nem DOM: só `.ts` nos imports, para os
// testes do Node rodarem direto. O Hub decide (nome, liberações, ficha); aqui só se anda de passo em passo.
import { Observable } from '../../../lib/observable.ts'
import { mercadoApi, type PedidoConectar } from './api.ts'
import { alternarGrupo, codigoDaRecusa, nomeSugerido, padraoSeguro, planoDeNomes, type CodigoDeRecusa } from './regras.ts'
import type { Conta, ProximoPasso, Servico, TipoConexao } from './tipos.ts'

const mensagem = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** O que o Hub e o login precisam para o fluxo andar; os testes entregam os seus. */
export interface Dependencias {
  cliente(): Promise<import('../../../client/host-client').HostClient>
  /** O login do navegador (`connectorSignIn` → `connectorSignInFinish`); volta quando a conta entrou ou a pessoa desistiu. */
  entrar(id: string): Promise<void>
  /** `connectorVerify`: devolve o estado que o Hub achou. */
  verificar(id: string): Promise<{ status: string }>
  /** `credentialUpdateConnector`: guarda a chave no cofre do Hub (nunca na tela). */
  guardarChave(id: string, campos: Record<string, string>): Promise<unknown>
  /** Relê catálogo e contas depois de cada escrita. */
  reler(): Promise<void>
}

/** O valor do cabeçalho: o prefixo da ficha (`Bearer `) mais a chave colada, sem repetir o prefixo se a pessoa já o colou. */
export const valorDoCabecalho = (prefixo: string | null | undefined, valor: string): string => {
  const chave = valor.trim()
  return prefixo && !chave.toLowerCase().startsWith(prefixo.trim().toLowerCase() + ' ') ? `${prefixo}${chave}` : chave
}

export type Passo = 'configurar' | 'entrando' | 'chave' | 'conferindo' | 'pronto'

export interface ErroDoFluxo {
  codigo: CodigoDeRecusa | null
  texto: string
  /** Em qual campo do nome o erro cai (o do nome novo, ou o da 1ª conta). */
  campo: 'nova' | 'primeira' | null
}

const CAMPO_DO_CODIGO: Partial<Record<CodigoDeRecusa, 'nova' | 'primeira'>> = {
  nome_obrigatorio: 'nova', nome_repetido: 'nova', nome_invalido: 'nova', nome_primeira_obrigatorio: 'primeira',
}

/**
 * Conectar uma conta pela ficha (telas 03 a 04): configurar, entrar (login ou chave), conferir e pronto. A conta nasce
 * no Hub ao confirmar; se a pessoa desistir antes do fim, a conta pela metade é desconectada.
 */
export class FluxoConectar extends Observable {
  passo: Passo = 'configurar'
  escolhidos: string[]
  personalizando = false
  nomeDaNova = ''
  nomeDaPrimeira = ''
  erro: ErroDoFluxo | null = null
  /** A chave que o fornecedor recusou, ou o login que falhou: o passo de "Entrar" fala disso. */
  falhaDeEntrada: string | null = null
  conta: Conta | null = null
  ocupado = false
  private proximo: ProximoPasso | null = null

  readonly servico: Servico
  readonly tipo: TipoConexao
  private readonly deps: Dependencias

  constructor(servico: Servico, tipo: TipoConexao, deps: Dependencias) {
    super()
    this.servico = servico
    this.tipo = tipo
    this.deps = deps
    this.escolhidos = padraoSeguro(tipo)
    // Da 2ª conta em diante o campo já vem com "Serviço 2" (editável); o Hub segue conferindo o nome.
    this.nomeDaNova = nomeSugerido(servico)
  }

  get nomes() { return planoDeNomes(this.servico) }

  /** Dá para confirmar a configuração: ao menos uma liberação e os nomes que a regra pede preenchidos. */
  get podeContinuar(): boolean {
    if (this.ocupado || this.escolhidos.length === 0) return false
    const n = this.nomes
    if (n.pedeNomeDaNova && this.nomeDaNova.trim() === '') return false
    if (n.pedeNomeDaPrimeira && this.nomeDaPrimeira.trim() === '') return false
    return true
  }

  alternar(grupo: string) { this.escolhidos = alternarGrupo(this.tipo, this.escolhidos, grupo); this.erro = null; this.changed() }
  voltarAoPadrao() { this.escolhidos = padraoSeguro(this.tipo); this.erro = null; this.changed() }
  personalizar() { this.personalizando = true; this.changed() }
  nomear(campo: 'nova' | 'primeira', valor: string) {
    if (campo === 'nova') this.nomeDaNova = valor
    else this.nomeDaPrimeira = valor
    if (this.erro?.campo === campo) this.erro = null
    this.changed()
  }

  private falhou(e: unknown): ErroDoFluxo {
    const codigo = codigoDaRecusa(e)
    return { codigo, texto: mensagem(e), campo: codigo ? (CAMPO_DO_CODIGO[codigo] ?? null) : null }
  }

  /** Passo 1 → 2: o Hub confere nome, ficha e liberações; só a recusa dele vira erro na tela. */
  async confirmar(): Promise<void> {
    if (!this.podeContinuar || this.passo !== 'configurar') return
    this.ocupado = true; this.erro = null; this.changed()
    try {
      const n = this.nomes
      const pedido: PedidoConectar = {
        servico: this.servico.id, tipo: this.tipo.id, liberacoes: this.escolhidos,
        ...(n.pedeNomeDaNova ? { nome: this.nomeDaNova.trim() } : {}),
        ...(n.pedeNomeDaPrimeira ? { nomePrimeira: this.nomeDaPrimeira.trim() } : {}),
      }
      const r = await mercadoApi.conectar(await this.deps.cliente(), pedido)
      this.conta = r.connector
      this.proximo = r.proximoPasso
      await this.deps.reler()
    } catch (e) {
      this.erro = this.falhou(e)
      this.ocupado = false; this.changed()
      return
    }
    this.ocupado = false
    await this.seguir()
  }

  /** O login (entrar) ou o campo da chave, conforme o Hub mandou. */
  private async seguir(): Promise<void> {
    if (this.proximo === 'chave') { this.passo = 'chave'; this.changed(); return }
    await this.entrar()
  }

  async entrar(): Promise<void> {
    if (!this.conta) return
    this.passo = 'entrando'; this.falhaDeEntrada = null; this.changed()
    try {
      await this.deps.entrar(this.conta.id)
    } catch (e) {
      this.falhaDeEntrada = mensagem(e); this.changed()
      return
    }
    await this.conferir()
  }

  async enviarChave(valor: string): Promise<void> {
    const conta = this.conta
    const auth = this.tipo.auth
    if (!conta || auth.modo !== 'chave' || valor.trim() === '') return
    this.falhaDeEntrada = null; this.ocupado = true; this.changed()
    try {
      await this.deps.guardarChave(conta.id, { [auth.nome]: valorDoCabecalho(auth.prefixo, valor) })
    } catch (e) {
      this.falhaDeEntrada = mensagem(e); this.ocupado = false; this.changed()
      return
    }
    this.ocupado = false
    await this.conferir()
  }

  /** Depois do login ou da chave: pergunta ao Hub se a conta funciona antes de dizer que está conectada. */
  private async conferir(): Promise<void> {
    const conta = this.conta
    if (!conta) return
    this.passo = 'conferindo'; this.changed()
    try {
      const r = await this.deps.verificar(conta.id)
      if (r.status !== 'ready') throw new Error(r.status)
      await this.deps.reler()
      this.passo = 'pronto'
    } catch (e) {
      this.falhaDeEntrada = mensagem(e)
      this.passo = this.proximo === 'chave' ? 'chave' : 'entrando'
    }
    this.changed()
  }

  /** Desistir antes do fim: a conta criada pela metade sai (só o registro dela), sem deixar conta fantasma. */
  async cancelar(): Promise<void> {
    const conta = this.conta
    if (!conta || this.passo === 'pronto') return
    this.conta = null
    // Hub ocupado ou link caído: uma segunda tentativa; se ainda assim falhar, a conta aparece em Minhas contas.
    for (let tentativa = 0; tentativa < 2; tentativa++) {
      try { await mercadoApi.desconectar(await this.deps.cliente(), conta.id); break } catch { /* tenta de novo; a leitura abaixo mostra o que ficou */ }
    }
    await this.deps.reler()
  }
}

