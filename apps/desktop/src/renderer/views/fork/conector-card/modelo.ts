// R6a: o cartão comum de ações de conector. Só lê o que o Hub entrega (`donnaConectorTentativa`) e só
// monta os corpos exatos de `donnaConectorDecidir`/`donnaConectorEditar` a partir do recibo do Hub.
// Nada aqui concede ou valida: quem decide se vale é o Hub (Caller Local, cartão, revisão, prazo).
import type { ConectorOperation } from '../../../../shared/models.ts'
import { ErroFrase, frase, type Frase } from '../../../lib/fork-i18n/frase.ts'

export type EstadoConector = ConectorOperation['state']

/** Recibo público do Hub (`Tentativa::receipt`, host/src/fork/conectores/operacao.rs). */
export interface ReciboConector {
  tentativaId: string
  snapshotId: string
  cardId: string
  cardRev: number
  requestDigest: string
  state: EstadoConector
  cardDeadline: number
  decisionProtocol: number
}

/** Protocolo de decisão que este app entende: o 2 traz a conta de saída e os rótulos curados. */
export const PROTOCOLO = 2

/** Conta de onde a ação sai, como a fonte canônica do Hub informou. Não é editável nem escolhida aqui. */
export interface ContaSaida {
  rotulo: string
  identidade: string
}

/**
 * Apresentação privada, só enquanto o pedido está aberto: a ação curada, a ferramenta e os argumentos
 * exatos, o rótulo curado de cada campo e a conta de saída (ou `null`, quando o Hub não a identificou).
 */
export interface ApresentacaoConector {
  efeito: string
  ferramenta: string
  argumentos: Record<string, string>
  rotulos: Record<string, string>
  conta: ContaSaida | null
}

export interface LeituraConector {
  recibo: ReciboConector
  apresentacao: ApresentacaoConector | null
}

const ESTADOS: readonly EstadoConector[] = ['aguardando', 'autorizada', 'em_envio', 'recusada', 'expirada', 'cancelada', 'concluida', 'falha_comprovada', 'incerta']
export const estadoConhecido = (s: unknown): s is EstadoConector => ESTADOS.includes(s as EstadoConector)

const obj = (v: unknown): Record<string, unknown> | null => (v && typeof v === 'object' && !Array.isArray(v) ? v as Record<string, unknown> : null)
const texto = (v: unknown) => (typeof v === 'string' && v.length > 0 ? v : null)

/**
 * Pedido aberto: `{receipt, presentation}`; encerrado: o recibo direto (api.rs). Forma inesperada é erro,
 * nunca um cartão montado pela metade.
 */
export function lerTentativa(raw: unknown): LeituraConector {
  const top = obj(raw)
  if (!top) throw new ErroFrase(frase('conectores.cartao.semRecibo'))
  const r = obj('receipt' in top ? top.receipt : top)
  const state = r?.state
  const ids = r && (['tentativaId', 'snapshotId', 'cardId', 'requestDigest'] as const).every((k) => texto(r[k]))
  if (!r || !ids || !estadoConhecido(state) || typeof r.cardRev !== 'number' || typeof r.cardDeadline !== 'number') throw new ErroFrase(frase('conectores.cartao.reciboIncompleto'))
  const recibo: ReciboConector = {
    tentativaId: r.tentativaId as string, snapshotId: r.snapshotId as string, cardId: r.cardId as string,
    cardRev: r.cardRev, requestDigest: r.requestDigest as string, state, cardDeadline: r.cardDeadline,
    decisionProtocol: typeof r.decisionProtocol === 'number' ? r.decisionProtocol : 0,
  }
  const p = 'receipt' in top ? obj(top.presentation) : null
  const args = obj(p?.argumentos)
  const rotulos = obj(p?.rotulos)
  // Cada campo mostrado precisa do rótulo curado do Hub; o app não inventa nome a partir da chave.
  const completa = p && texto(p.efeito) && texto(p.ferramenta) && args && rotulos &&
    Object.values(args).every((v) => typeof v === 'string') && Object.keys(args).every((k) => texto(rotulos[k]))
  const c = obj(p?.conta)
  const conta = c && texto(c.rotulo) && texto(c.identidade) ? { rotulo: c.rotulo as string, identidade: c.identidade as string } : null
  const apresentacao = completa
    ? { efeito: p.efeito as string, ferramenta: p.ferramenta as string, argumentos: args as Record<string, string>, rotulos: rotulos as Record<string, string>, conta }
    : null
  return { recibo, apresentacao }
}

/** Corpo exato de `donnaConectorDecidir`; os IDs vêm do recibo lido, nunca da entrada pública. */
export function corpoDecisao(r: ReciboConector, decisao: 'aprovar_uma_vez' | 'recusar') {
  return { protocol: PROTOCOLO, tentativaId: r.tentativaId, snapshotId: r.snapshotId, cardId: r.cardId, cardRev: r.cardRev, requestDigest: r.requestDigest, decisao, source: 'app' }
}

/** Só os campos mudados vão no patch; o Hub recusa os não editáveis e gera cartão novo para o resto. */
export function patchEdicao(antes: Record<string, string>, depois: Record<string, string>): Record<string, string> | null {
  const patch = Object.fromEntries(Object.entries(depois).filter(([k, v]) => k in antes && antes[k] !== v))
  return Object.keys(patch).length ? patch : null
}

export function corpoEdicao(r: ReciboConector, patch: Record<string, string>) {
  return { protocol: PROTOCOLO, tentativaId: r.tentativaId, snapshotId: r.snapshotId, cardId: r.cardId, cardRev: r.cardRev, requestDigest: r.requestDigest, source: 'app', patch }
}

export type Tom = 'espera' | 'ok' | 'neutro' | 'alerta'

// D454 (fatia 8b): selo e nota são frases do catálogo (`conectores.cartao.*`), traduzidas ao desenhar.
export interface Tela {
  selo: Frase
  tom: Tom
  /** Uma frase do que aconteceu ou acontece agora; `null`, nada a dizer. */
  nota: Frase | null
  /** Mostra Segure/Editar/Recusar. Só com o pedido aberto, cartão atual, no prazo e com a apresentação do Hub. */
  decide: boolean
}

export interface Contexto {
  /** O app entende este protocolo de decisão. */
  suportado: boolean
  apresentacao: boolean
  /** O Hub identificou a conta de saída na apresentação. Sem ela, nada de aprovar. */
  conta: boolean
  /** O prazo do cartão (wall clock do Hub) já passou. */
  vencido: boolean
  /** O cartão desta entrada não é o atual da tentativa (uma edição gerou outro). */
  substituido: boolean
  /** O Hub recusou abrir o pedido (por exemplo, app fora do computador do Hub); a mensagem é dele. */
  recusaLeitura?: Frase | null
}

const ENCERRADO: Record<Exclude<EstadoConector, 'aguardando'>, Omit<Tela, 'decide'>> = {
  autorizada: { selo: frase('conectores.cartao.selo.aprovado'), tom: 'ok', nota: frase('conectores.cartao.nota.aprovado') },
  em_envio: { selo: frase('conectores.cartao.selo.enviando'), tom: 'espera', nota: frase('conectores.cartao.nota.enviando') },
  concluida: { selo: frase('conectores.cartao.selo.concluido'), tom: 'ok', nota: frase('conectores.cartao.nota.concluido') },
  falha_comprovada: { selo: frase('conectores.cartao.selo.falhou'), tom: 'alerta', nota: frase('conectores.cartao.nota.falhou') },
  incerta: { selo: frase('conectores.cartao.selo.incerto'), tom: 'alerta', nota: frase('conectores.cartao.nota.incerto') },
  recusada: { selo: frase('conectores.cartao.selo.recusado'), tom: 'neutro', nota: frase('conectores.cartao.nota.nadaEnviado') },
  expirada: { selo: frase('conectores.cartao.selo.venceu'), tom: 'neutro', nota: frase('conectores.cartao.nota.expirada') },
  cancelada: { selo: frase('conectores.cartao.selo.cancelado'), tom: 'neutro', nota: frase('conectores.cartao.nota.nadaEnviado') },
}

/** Estado do Hub → o que a tela mostra. Botão só aparece quando pode funcionar. */
const FINAIS: readonly EstadoConector[] = ['recusada', 'expirada', 'cancelada', 'concluida', 'falha_comprovada', 'incerta']

export function tela(estado: EstadoConector | string, c: Contexto): Tela {
  // Um fim já registrado pelo Hub (inclusive recibo antigo, de outro protocolo) só é relatado, sem botão.
  if (estadoConhecido(estado) && FINAIS.includes(estado) && estado !== 'aguardando') return { ...ENCERRADO[estado], decide: false }
  if (!c.suportado || !estadoConhecido(estado)) {
    return { selo: frase('conectores.cartao.selo.naoDecide'), tom: 'neutro', nota: frase('conectores.cartao.nota.naoSuportado'), decide: false }
  }
  if (estado !== 'aguardando') return { ...ENCERRADO[estado], decide: false }
  if (c.substituido) return { selo: frase('conectores.cartao.selo.substituido'), tom: 'neutro', nota: frase('conectores.cartao.nota.substituido'), decide: false }
  if (c.vencido) return { selo: frase('conectores.cartao.selo.venceu'), tom: 'neutro', nota: frase('conectores.cartao.nota.venceu'), decide: false }
  if (c.recusaLeitura) return { selo: frase('conectores.cartao.selo.naoDecide'), tom: 'neutro', nota: frase('conectores.cartao.nota.recusaLeitura', { motivo: c.recusaLeitura }), decide: false }
  if (!c.apresentacao) return { selo: frase('conectores.cartao.selo.esperando'), tom: 'espera', nota: frase('conectores.cartao.nota.semApresentacao'), decide: false }
  if (!c.conta) return { selo: frase('conectores.cartao.selo.esperando'), tom: 'alerta', nota: frase('conectores.cartao.nota.semConta'), decide: false }
  return { selo: frase('conectores.cartao.selo.esperando'), tom: 'espera', nota: null, decide: true }
}

/** A chamada ao Hub (`HostClient.call`), estreita para o teste. */
export type Chamar = (metodo: string, corpo: object) => Promise<unknown>

export const lerPedido = async (chamar: Chamar, tentativaId: string) => lerTentativa(await chamar('donnaConectorTentativa', { tentativaId }))

export const decidir = async (chamar: Chamar, r: ReciboConector, decisao: 'aprovar_uma_vez' | 'recusar') =>
  lerTentativa(await chamar('donnaConectorDecidir', corpoDecisao(r, decisao))).recibo

/** Sem mudança, não chama o Hub (`null`). */
export async function editar(chamar: Chamar, r: ReciboConector, antes: Record<string, string>, depois: Record<string, string>) {
  const patch = patchEdicao(antes, depois)
  return patch ? lerTentativa(await chamar('donnaConectorEditar', corpoEdicao(r, patch))).recibo : null
}
