// DON-906 (C3): o ajuste de compressão do Hub (Ajustes → Conversa → Contexto). Vale para o Hub inteiro, não é
// preferência do aparelho: lido com `donnaContextSettings {}` e gravado com `donnaContextSettingsSave` (mesmo DTO).
// Quem pode mudar é o Hub que decide (dono local ou aparelho com Control); aqui só a forma e o texto das recusas.
// Sem texto: só códigos; a view traduz com `useT` (`contexto.ajustes.*`, docs/reference/design/don-906-c3/README.md).

export const RPC_AJUSTES = 'donnaContextSettings'
export const RPC_SALVAR = 'donnaContextSettingsSave'

/** O piso seguro (plano, seção 2): a própria compressão precisa de espaço. */
export const MINIMO_MIL = 50

export type Parada = 'beforeExpiry' | 'never'

/** `triggerTokens` null = Automático (regra do Hub: 75% da janela, no máximo 200 mil); número: ≥ 50 mil, múltiplo de 1000. */
export interface AjustesContexto { triggerTokens: number | null; idle: Parada }

/** Confere a resposta do Hub; fora da forma, null (nada é mostrado como se fosse do Hub). */
export function conferirAjustes(raw: unknown): AjustesContexto | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (r.idle !== 'beforeExpiry' && r.idle !== 'never') return null
  const t = r.triggerTokens
  if (t !== null && !(typeof t === 'number' && Number.isSafeInteger(t) && t >= MINIMO_MIL * 1000 && t % 1000 === 0)) return null
  return { triggerTokens: t, idle: r.idle }
}

/** Tokens → o número do campo, em mil (150000 → "150"). */
export const paraCampo = (tokens: number) => String(Math.round(tokens / 1000))

/** Por que o campo não vale. Código estável: a view traduz (`contexto.ajustes.erro.*`). */
export type ErroCampo = 'numero' | 'minimo' | 'grande'

/**
 * O que a pessoa digitou (em mil tokens) → tokens, ou o código do erro. Aceita "150", "1.000", "1,000" e "1 000"
 * (separador de milhar de qualquer idioma, sempre em grupos de 3); "1,5" não é número inteiro e é recusado.
 */
export function lerCampo(texto: string): { tokens: number } | { erro: ErroCampo } {
  const t = texto.trim()
  if (!/^(\d+|\d{1,3}([.,\s\u00a0\u202f'\u066c]\d{3})+)$/.test(t)) return { erro: 'numero' }
  const mil = Number(t.replace(/\D/g, ''))
  if (mil < MINIMO_MIL) return { erro: 'minimo' }
  const tokens = mil * 1000
  if (!Number.isSafeInteger(tokens)) return { erro: 'grande' }
  return { tokens }
}

/** Por que ler ou salvar falhou. Código estável: a view traduz (`contexto.ajustes.falha.*`). */
export type Falha = 'hubAntigo' | 'soDono' | 'recusado' | 'ler' | 'salvar' | 'resposta'

/** Hub antigo ou sem autoridade: estados fixos, sem "Tentar de novo". */
export const falhaFixa = (f: Falha) => f === 'hubAntigo' || f === 'soDono'

const status = (e: unknown) => (typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null)
const mensagem = (e: unknown) => (typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e))

export const semRpc = (e: unknown) => status(e) === 404 || /unknown method|Not available in the web demo/i.test(mensagem(e))
export const semAutoridade = (e: unknown) => status(e) === 401 || status(e) === 403 || /forbidden|not_owner|unauthorized/i.test(mensagem(e))

/** A recusa como código, nunca o texto cru do Hub. Só o 400 ao salvar afirma que nada mudou. */
export function codigoFalha(e: unknown, acao: 'ler' | 'salvar'): Falha {
  if (semRpc(e)) return 'hubAntigo'
  if (semAutoridade(e)) return 'soDono'
  if (acao === 'salvar' && status(e) === 400) return 'recusado'
  // Falha genérica ao salvar (rede, tempo): o Hub pode ter gravado; o componente relê o valor real.
  return acao
}
