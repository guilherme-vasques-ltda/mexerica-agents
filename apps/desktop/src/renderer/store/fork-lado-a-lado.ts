// Loja do quadro Lado a lado: guarda o mapa por escopo, grava em disco (fail-safe: erro de
// quota ou de JSON nunca derruba o quadro), sincroniza entre abas pelo evento de storage e
// avisa limite e remoções. As regras vêm todas de shared/fork-lado-a-lado.ts; aqui só estado.
import { Observable } from '../lib/observable.ts'
import {
  CHAVE_ARMAZENAMENTO_LADO_A_LADO, MAX_PAINEIS, abrirNoQuadro, atualizarSessaoPainel, ciclarFocoPainel,
  definirAtivo, escopoDoSpace, estadoVazio, focarPainel, mesmaRefLadoALado, moverPainel, parseLadoALadoMapa,
  reconciliarEscopo, removerPainel, trocarPainel,
  type ConversaDisponivel, type LadoALadoEscopo, type LadoALadoEscopoEstado, type LadoALadoMapa, type LadoALadoRef,
  type ResultadoAbrirPainel,
} from '../../shared/fork-lado-a-lado.ts'

/** Aviso de uma tentativa que o quadro não pôde atender; a view mostra e chama `limparAviso`. */
export type LadoALadoAviso = { kind: 'limite'; maximo: number } | { kind: 'removidas'; count: number }

/** Vazio compartilhado: `estado()` de um escopo inédito devolve sempre a mesma referência. */
const ESTADO_VAZIO = estadoVazio()

export class LadoALadoStore extends Observable {
  private mapa: LadoALadoMapa
  private avisoAtual: LadoALadoAviso | null = null
  private readonly storage: Pick<Storage, 'getItem' | 'setItem'> | null

  constructor(storage?: Pick<Storage, 'getItem' | 'setItem'> | null) {
    super()
    this.storage = storage === undefined ? (typeof localStorage !== 'undefined' ? localStorage : null) : storage
    this.mapa = this.lerDoDisco()
    if (this.storage && typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
      window.addEventListener('storage', (evento) => {
        if (evento.key !== null && evento.key !== CHAVE_ARMAZENAMENTO_LADO_A_LADO) return
        this.mapa = this.lerDoDisco()
        this.changed()
      })
    }
  }

  /** Estado do quadro no escopo; escopo sem nada devolve o vazio compartilhado. */
  estado(escopo: LadoALadoEscopo | null): LadoALadoEscopoEstado {
    return this.mapa[escopoDoSpace(escopo)] ?? ESTADO_VAZIO
  }

  get aviso(): LadoALadoAviso | null {
    return this.avisoAtual
  }

  limparAviso(): void {
    if (this.avisoAtual === null) return
    this.avisoAtual = null
    this.changed()
  }

  /** Entra no quadro do escopo: restaura painéis salvos; sem nenhum, abre a conversa inicial dada. */
  entrar(escopo: LadoALadoEscopo | null, inicial: LadoALadoRef | null, sessionId?: string | null): void {
    this.aplicar(escopo, (atual) => {
      if (atual.paineis.length >= 1) return definirAtivo(atual, true)
      if (!inicial) return atual
      return abrirNoQuadro(atual, inicial, escopo, { sessionId: sessionId ?? null, ativar: true }).estado
    })
  }

  /** Sai do quadro do escopo, sem perder os painéis. */
  sair(escopo: LadoALadoEscopo | null): void {
    this.aplicar(escopo, (atual) => definirAtivo(atual, false))
  }

  /** Entra se fechado, sai se aberto. */
  alternar(escopo: LadoALadoEscopo | null, inicial: LadoALadoRef | null, sessionId?: string | null): void {
    if (this.estado(escopo).ativo) this.sair(escopo)
    else this.entrar(escopo, inicial, sessionId)
  }

  /** Abre uma conversa no quadro do escopo. Quadro vazio com conversa inicial diferente entrega os
   *  dois painéis (inicial primeiro, conversa pedida no foco); cheio, avisa limite. */
  abrir(
    escopo: LadoALadoEscopo | null,
    ref: LadoALadoRef,
    opcoes?: { sessionId?: string | null; inicial?: LadoALadoRef | null; inicialSessionId?: string | null },
  ): ResultadoAbrirPainel {
    const chave = escopoDoSpace(escopo)
    let atual = this.mapa[chave] ?? ESTADO_VAZIO
    const sessionId = opcoes?.sessionId ?? null
    const inicialSessionId = opcoes?.inicialSessionId ?? null
    if (atual.paineis.length === 0 && opcoes?.inicial
      && (!mesmaRefLadoALado(opcoes.inicial, ref) || inicialSessionId !== sessionId)) {
      atual = abrirNoQuadro(atual, opcoes.inicial, escopo, { sessionId: inicialSessionId, ativar: true }).estado
    }
    const { estado, resultado } = abrirNoQuadro(atual, ref, escopo, { sessionId, ativar: true })
    this.gravarEstado(chave, estado)
    if (resultado === 'limite') this.marcarAviso({ kind: 'limite', maximo: MAX_PAINEIS })
    return resultado
  }

  remover(escopo: LadoALadoEscopo | null, panelId: string): void {
    this.aplicar(escopo, (atual) => removerPainel(atual, panelId))
  }

  trocar(escopo: LadoALadoEscopo | null, panelId: string, ref: LadoALadoRef, sessionId?: string | null): void {
    this.aplicar(escopo, (atual) => trocarPainel(atual, panelId, ref, escopo, sessionId ?? null))
  }

  atualizarSessao(escopo: LadoALadoEscopo | null, panelId: string, sessionId: string | null): void {
    this.aplicar(escopo, (atual) => atualizarSessaoPainel(atual, panelId, sessionId))
  }

  mover(escopo: LadoALadoEscopo | null, panelId: string, delta: -1 | 1): void {
    this.aplicar(escopo, (atual) => moverPainel(atual, panelId, delta))
  }

  focar(escopo: LadoALadoEscopo | null, panelId: string): void {
    this.aplicar(escopo, (atual) => focarPainel(atual, panelId))
  }

  /** Cicla o foco e devolve o painel focado depois do giro. */
  ciclarFoco(escopo: LadoALadoEscopo | null, delta: -1 | 1): string | null {
    const chave = escopoDoSpace(escopo)
    const proximo = ciclarFocoPainel(this.mapa[chave] ?? ESTADO_VAZIO, delta)
    this.gravarEstado(chave, proximo)
    return proximo.focoId
  }

  /** Ajusta o quadro ao que existe; devolve quantas conversas sumiram e viraram aviso quando o
   *  quadro estava aberto. */
  reconciliar(escopo: LadoALadoEscopo | null, disponiveis: readonly ConversaDisponivel[], computadoresProntos: ReadonlySet<string>): number {
    const chave = escopoDoSpace(escopo)
    const antes = this.mapa[chave] ?? ESTADO_VAZIO
    const { estado, removidos } = reconciliarEscopo(antes, escopo, disponiveis, computadoresProntos)
    this.gravarEstado(chave, estado)
    if (removidos > 0 && antes.ativo) this.marcarAviso({ kind: 'removidas', count: removidos })
    return removidos
  }

  private aplicar(escopo: LadoALadoEscopo | null, fn: (estado: LadoALadoEscopoEstado) => LadoALadoEscopoEstado): void {
    const chave = escopoDoSpace(escopo)
    this.gravarEstado(chave, fn(this.mapa[chave] ?? ESTADO_VAZIO))
  }

  private gravarEstado(chave: string, proximo: LadoALadoEscopoEstado): void {
    const atual = this.mapa[chave] ?? ESTADO_VAZIO
    if (proximo === atual) return
    this.mapa = { ...this.mapa, [chave]: proximo }
    try { this.storage?.setItem(CHAVE_ARMAZENAMENTO_LADO_A_LADO, JSON.stringify(this.mapa)) } catch {}
    this.changed()
  }

  private marcarAviso(aviso: LadoALadoAviso): void {
    this.avisoAtual = aviso
    this.changed()
  }

  private lerDoDisco(): LadoALadoMapa {
    if (!this.storage) return {}
    try {
      const raw = this.storage.getItem(CHAVE_ARMAZENAMENTO_LADO_A_LADO)
      if (!raw) return {}
      return parseLadoALadoMapa(JSON.parse(raw))
    } catch {
      return {}
    }
  }
}

/** A loja única do app; no processo do renderer grava em `localStorage`. */
export const ladoALadoStore = new LadoALadoStore()
