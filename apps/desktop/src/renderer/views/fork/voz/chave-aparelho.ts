import type { CategoriaChave, ChaveAparelho, EstadoChaves, ResultadoChave, SituacaoChave, VozBridge } from '../../../../shared/fork-voz.ts'
import type { FonteChave, VozProvedor } from './voz-model.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'

// voz-04 (plano §2): regras puras da chave guardada neste aparelho. Ordem das fontes, por modo: a chave
// deste aparelho, depois a do Hub do bot, depois sem chave. Até o voz-05b, a chave deste aparelho só
// vale no Tempo real; no modo Fala, ela impede a reserva do Hub em cada parte (ouvir, ler) separada.
// A chave digitada sai do campo antes de qualquer espera e não volta em texto nenhum.

/** `tempo-real`: o Ao vivo em Tempo real. `ouvir`: a Voz para texto. `ler`: o provedor que lê no modo Fala. */
export type ModoVoz = 'tempo-real' | 'ouvir' | 'ler'

/**
 * Quem paga, de fato, no modo: `aparelho` (chave deste aparelho), `hub` (reserva do Hub do bot), `nenhuma`,
 * `nao-abriu` (chave daqui travada ou recusada, sem escolha da reserva: segue local ou para, nunca o Hub),
 * `so-tempo-real` (modo que ainda não usa a chave), `incerta`/`limpeza-pendente` (disco sem confirmação)
 * e `desconhecida`. Nenhum desses bloqueios passa sozinho ao Hub.
 */
export type Fonte = 'aparelho' | 'hub' | 'nenhuma' | 'nao-abriu' | 'so-tempo-real' | 'desconhecida' | 'incerta' | 'limpeza-pendente'

/** A chave deste aparelho decide esse provedor (paga ou bloqueia a reserva), a menos que a pessoa escolheu o Hub. */
export const chaveDaquiDecide = (c: ChaveAparelho | null | undefined) => !!c && c.situacao !== 'ausente' && !c.reserva

export function fonteEfetiva(modo: ModoVoz, aparelho: ChaveAparelho | null | undefined, hub: FonteChave): Fonte {
  if (chaveDaquiDecide(aparelho)) {
    if (aparelho!.situacao === 'incerta' || aparelho!.situacao === 'limpeza-pendente') return aparelho!.situacao
    if (modo !== 'tempo-real') return 'so-tempo-real'
    return aparelho!.situacao === 'guardada' ? 'aparelho' : 'nao-abriu'
  }
  return hub === 'hub' ? 'hub' : hub === 'nenhuma' ? 'nenhuma' : 'desconhecida'
}

// MARK: textos da transição (texto aprovado pelo subcoordenador; plano §2 e S9). Fatia 6b (D454): como dado (D507);
// as frases ficam no catálogo voz/ajustes e `{neste}`/`{deste}` entram ao desenhar (frases.ts, `textoVoz`).

export type ParteLocal = 'ouvir' | 'ler' | 'ambos'

/** Mac: a parte com chave deste Mac segue no próprio Mac. */
export const textoTransicaoMac = (parte: ParteLocal): ChaveTexto => `voz.ajustes.transicao.${parte}`

/** Windows/Linux (e Mac sem Ditado): avisa e não liga. A segunda saída só quando o Hub do bot tem chave. */
export function textoTransicaoSemLocal(hubComChave: string | null): Mensagem {
  return hubComChave ? { chave: 'voz.ajustes.transicao.semLocalHub' } : { chave: 'voz.ajustes.transicao.semLocal' }
}

export const TEXTO_SEM_COFRE = 'voz.ajustes.semCofre' satisfies ChaveTexto

// MARK: ações da tela

/** O resultado de uma ação: frases inteiras do catálogo, uma depois da outra (D507: dado, traduzido ao desenhar). */
export interface ResultadoTela { ok: boolean; partes: Mensagem[] }

export const textoSituacaoPendente = (situacao: SituacaoChave | null): ChaveTexto | null =>
  situacao === 'incerta' ? 'voz.ajustes.resultado.pendenteIncerta'
    : situacao === 'limpeza-pendente' ? 'voz.ajustes.resultado.pendenteLimpeza' : null

type Acao = 'guardar' | 'limpar' | 'testar' | 'reserva'

const so = (chave: ChaveTexto, params?: Record<string, string>): Mensagem[] => [params ? { chave, params } : { chave }]

/** Texto leigo de cada resposta do main; nada do provedor, só a categoria. */
export function textoDoResultado(acao: Acao, r: Pick<ResultadoChave, 'ok' | 'categoria'>, hub: string | null, situacao: SituacaoChave | null = null): ResultadoTela {
  if (r.ok) {
    const partes = acao === 'guardar' ? so('voz.ajustes.resultado.guardou')
      : acao === 'limpar' ? (hub ? so('voz.ajustes.resultado.apagouHub') : so('voz.ajustes.resultado.apagou'))
        : acao === 'testar' ? so('voz.ajustes.resultado.funciona')
          : hub ? so('voz.ajustes.resultado.reservaHub') : so('voz.ajustes.resultado.reserva')
    return { ok: true, partes }
  }
  const pendente = r.categoria === 'registro' ? null : textoSituacaoPendente(situacao)
  if (pendente) return { ok: false, partes: so(pendente) }
  const nada: Mensagem[] = acao === 'guardar' ? so('voz.ajustes.resultado.nadaGuardado') : []
  const provedor = (c: 'chave' | 'saldo' | 'limite' | 'rede' | 'provedor'): Mensagem[] => [{ chave: `voz.chamada.falha.${c}` }, ...nada]
  const porCategoria: Record<CategoriaChave, Mensagem[]> = {
    chave: provedor('chave'),
    saldo: provedor('saldo'),
    limite: provedor('limite'),
    rede: provedor('rede'),
    provedor: provedor('provedor'),
    cofre: acao === 'guardar' ? so('voz.ajustes.resultado.cofreGuardar')
      : acao === 'limpar' ? so('voz.ajustes.resultado.pendenteLimpeza')
        : so('voz.ajustes.resultado.cofreAbrir'),
    cancelado: so('voz.ajustes.resultado.cancelado'),
    registro: so('voz.ajustes.resultado.registro'),
    'sem-cofre': so('voz.ajustes.resultado.semCofre'),
    'sem-chave': so('voz.ajustes.resultado.semChave'),
    invalida: acao === 'guardar' ? so('voz.ajustes.resultado.invalida') : so('voz.ajustes.resultado.agora'),
    origem: so('voz.ajustes.resultado.agora'),
  }
  return { ok: false, partes: porCategoria[r.categoria ?? 'provedor'] }
}

export interface Respondido extends ResultadoTela { estado: EstadoChaves | null }

/**
 * Guarda a chave neste aparelho. `limparCampo` roda antes de qualquer espera: a chave não fica no estado
 * da tela enquanto o main confere com o provedor. O texto devolvido nunca contém a chave.
 */
export async function guardarNoAparelho(ponte: Pick<VozBridge, 'guardar'>, p: VozProvedor, digitada: string, limparCampo: () => void): Promise<Respondido> {
  let chave = digitada.trim()
  limparCampo()
  if (!chave) return { ok: false, partes: so('voz.ajustes.resultado.cole'), estado: null }
  try {
    const r = await ponte.guardar(p, chave)
    return { ...textoDoResultado('guardar', r, null, r.estado.chaves[p].situacao), estado: r.estado }
  } catch {
    return { ...textoDoResultado('guardar', { ok: false, categoria: 'provedor' }, null), estado: null }
  } finally {
    chave = ''
  }
}

/** Limpar, Testar/Tentar de novo e "Usar a chave do Hub": só o provedor vai ao main. */
export async function agirNoAparelho(ponte: Pick<VozBridge, 'limpar' | 'testar' | 'usarReserva'>, acao: Exclude<Acao, 'guardar'>, p: VozProvedor, hub: string | null): Promise<Respondido> {
  try {
    const r = await (acao === 'limpar' ? ponte.limpar(p) : acao === 'testar' ? ponte.testar(p) : ponte.usarReserva(p))
    return { ...textoDoResultado(acao, r, hub, r.estado.chaves[p].situacao), estado: r.estado }
  } catch {
    return { ...textoDoResultado(acao, { ok: false, categoria: 'provedor' }, hub), estado: null }
  }
}
