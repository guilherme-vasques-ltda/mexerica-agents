// voz-03 (D70): toda falha de provedor vira categoria + texto leigo antes de chegar ao motor, à tela ou ao
// log. O texto bruto do provedor (que pode ecoar a chave, um token curto ou o corpo da resposta) nunca é
// guardado: nem em `failure`, nem em estado, nem na mensagem de erro que segue adiante.

import type { CategoriaChave } from '../../../../shared/fork-voz.ts'
import { CATALOGOS, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { ErroComChave, textoAgora, type Mensagem } from '../bot-settings/mensagem.ts'

export type CategoriaFalha = 'chave' | 'saldo' | 'limite' | 'rede' | 'provedor' | 'cofre'

/** Fatia 6b (D454): o texto leigo de cada categoria fica no catálogo (voz/chamada); aqui, só a chave. */
export const CHAVES_FALHA: Record<CategoriaFalha, ChaveTexto> = {
  chave: 'voz.chamada.falha.chave',
  saldo: 'voz.chamada.falha.saldo',
  limite: 'voz.chamada.falha.limite',
  rede: 'voz.chamada.falha.rede',
  provedor: 'voz.chamada.falha.provedor',
  /** voz-04: a chave guardada neste aparelho não abriu (cofre do sistema) ou saiu no meio. */
  cofre: 'voz.chamada.falha.cofre',
}

/** Erro já seguro: só a categoria; `message` sai no idioma da hora, para quem só lê texto. */
export class FalhaVoz extends Error {
  readonly categoria: CategoriaFalha
  readonly chave: ChaveTexto
  constructor(categoria: CategoriaFalha) {
    super(textoAgora({ chave: CHAVES_FALHA[categoria] }))
    this.name = 'FalhaVoz'
    this.categoria = categoria
    this.chave = CHAVES_FALHA[categoria]
  }
}

/** A chamada acabou: o pedido não sai (ou a resposta atrasada não é usada). Não é falha do provedor. */
export class PedidoCancelado extends Error {
  readonly chave: ChaveTexto = 'voz.chamada.falha.encerrada'
  constructor() {
    super(textoAgora({ chave: 'voz.chamada.falha.encerrada' }))
    this.name = 'PedidoCancelado'
  }
}

/** Só a categoria, lida do texto bruto; o texto em si é descartado por quem chama. */
export function categoriaDe(bruto: string): CategoriaFalha {
  const t = bruto.toLowerCase()
  if (/\b(401|403)\b|api[ _-]?key|invalid[ _-]?(key|auth)|unauthori[sz]ed|permission|forbidden|credential|chave/.test(t)) return 'chave'
  if (/\b402\b|quota|billing|insufficient|credit|saldo|cota/.test(t)) return 'saldo'
  if (/\b429\b|rate[ _-]?limit|too many|resource[ _-]?exhausted/.test(t)) return 'limite'
  if (/network|fetch|timeout|timed out|econn|enotfound|offline|unreachable|socket|conex|alcan|tempo esgotado/.test(t)) return 'rede'
  return 'provedor'
}

/** voz-04: a categoria que o main devolve vira a falha segura do motor; nada do provedor chega aqui. */
export function falhaDaCategoria(c: CategoriaChave): FalhaVoz {
  switch (c) {
    case 'chave': case 'saldo': case 'limite': case 'rede': case 'provedor': return new FalhaVoz(c)
    case 'cofre': case 'registro': case 'cancelado': case 'sem-cofre': case 'sem-chave': return new FalhaVoz('cofre')
    default: return new FalhaVoz('provedor')
  }
}


/** Converte qualquer erro de provedor num `FalhaVoz`. Cancelamento passa como está. */
export function falhaSegura(e: unknown): FalhaVoz | PedidoCancelado {
  if (e instanceof FalhaVoz || e instanceof PedidoCancelado) return e
  const bruto = e instanceof Error ? e.message : String(e)
  return new FalhaVoz(categoriaDe(bruto))
}

/** Formatos de chave e de token curto (OpenAI `sk-`/`ek_`, Google `AIza`/`auth_tokens/`, Bearer). */
const FORMA_DE_SEGREDO = /\b(sk-[A-Za-z0-9_-]{6,}|ek_[A-Za-z0-9_-]{6,}|AIza[0-9A-Za-z_-]{10,}|auth_tokens\/[A-Za-z0-9_-]+)|Bearer\s+\S+|openai-insecure-api-key\.\S+|xai-client-secret\.\S+|xai-realtime-client-secret-[A-Za-z0-9._~-]+|xai-[A-Za-z0-9_-]{6,}|xi-api-key[\s:=]+\S+/g

/** O texto de cada falha conhecida, nos 6 idiomas, de volta à chave (um `message` já traduzido volta a ser dado). */
let conhecidas: Map<string, ChaveTexto> | null = null
function chaveConhecida(texto: string): ChaveTexto | null {
  if (!conhecidas) {
    conhecidas = new Map()
    const chaves = Object.keys(CATALOGOS['pt-BR']).filter((c) => c.startsWith('voz.chamada.falha.') || c === 'marca.voz.microfone') as ChaveTexto[]
    for (const cat of Object.values(CATALOGOS)) for (const c of chaves) { const v = (cat as Record<string, string | undefined>)[c]; if (v) conhecidas.set(v, c) }
  }
  return conhecidas.get(texto) ?? null
}

/**
 * Rede de segurança da falha do motor (gancho em `call/voice.ts`), como dado (D507): falha conhecida vira a chave; um
 * texto com forma de segredo vira só a categoria; o resto (microfone, ditado, o que o sistema diz) passa como texto.
 */
export function mensagemSegura(texto: string): Mensagem {
  const conhecida = chaveConhecida(texto)
  if (conhecida) return { chave: conhecida }
  FORMA_DE_SEGREDO.lastIndex = 0
  return FORMA_DE_SEGREDO.test(texto) ? { chave: CHAVES_FALHA[categoriaDe(texto)] } : { texto }
}

/** Um erro que já traz a mensagem como dado (chave e parâmetros); `message` sai no idioma da hora. */
export class ErroComMensagem extends Error {
  readonly mensagem: Mensagem
  constructor(mensagem: Mensagem) {
    super(textoAgora(mensagem))
    this.mensagem = mensagem
  }
}

/** Qualquer motivo de falha (mensagem, erro com chave, erro ou texto) como dado seguro. */
export function motivoDaFalha(motivo: unknown): Mensagem {
  if (motivo instanceof FalhaVoz || motivo instanceof PedidoCancelado || motivo instanceof ErroComChave) return { chave: motivo.chave }
  if (motivo instanceof ErroComMensagem) return 'chave' in motivo.mensagem ? motivo.mensagem : mensagemSegura(motivo.mensagem.texto)
  if (motivo && typeof motivo === 'object' && ('chave' in motivo || 'texto' in motivo) && !(motivo instanceof Error)) {
    const m = motivo as Mensagem
    return 'chave' in m ? m : mensagemSegura(m.texto)
  }
  return mensagemSegura(motivo instanceof Error ? motivo.message : String(motivo))
}
