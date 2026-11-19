import type { CategoriaChave, VozProvedorAparelho } from '../../shared/fork-voz.ts'

// voz-04 (plano §2): o que o main pede ao provedor com a chave deste aparelho. Só o código da resposta
// (e, para classificar, o corpo lido aqui e descartado) decide a categoria; nada da resposta de erro sai
// daqui. A credencial curta é a mesma que o Hub emite (`host/src/voice.rs`, `mint`).

export interface Resposta { status: number; text(): Promise<string> }
export type Buscar = (url: string, init: { method: string; headers: Record<string, string>; body?: string; signal?: AbortSignal }) => Promise<Resposta>

export type Resultado<T> = { ok: true; valor: T } | { ok: false; categoria: CategoriaChave }

const ESPERA_MS = 15_000

/** A categoria de uma recusa, pelo código e por palavras do corpo; o corpo é descartado por quem chama. */
export function categoriaDaResposta(status: number, corpo: string): CategoriaChave {
  if (status === 401 || status === 403) return 'chave'
  if (status === 402) return 'saldo'
  if (status === 429) return 'limite'
  // O Gemini responde 400 para chave inválida; o corpo só serve para distinguir isso.
  const t = corpo.toLowerCase()
  if (/api[ _-]?key[ _-]?(not[ _-]?valid|invalid)|invalid[ _-]?api[ _-]?key|api_key_invalid|incorrect api key/.test(t)) return 'chave'
  if (/insufficient_quota|billing|quota/.test(t)) return 'saldo'
  return 'provedor'
}

async function pedir(buscar: Buscar, url: string, init: { method: string; headers: Record<string, string>; body?: string }): Promise<Resultado<string>> {
  let resposta: Resposta
  try {
    resposta = await buscar(url, { ...init, signal: AbortSignal.timeout(ESPERA_MS) })
  } catch {
    return { ok: false, categoria: 'rede' }
  }
  let corpo = ''
  try { corpo = await resposta.text() } catch { /* corpo ilegível: decide só o código */ }
  if (resposta.status < 200 || resposta.status >= 300) return { ok: false, categoria: categoriaDaResposta(resposta.status, corpo) }
  return { ok: true, valor: corpo }
}

const cabecalhos = (p: Exclude<VozProvedorAparelho, 'xai'>, chave: string): Record<string, string> =>
  p === 'openai' ? { Authorization: `Bearer ${chave}` } : { 'x-goog-api-key': chave }

/**
 * Confere a chave sem abrir sessão de áudio e sem custo: a lista de modelos do provedor. Não depende do
 * modelo escolhido (um modelo que sumiu não faz uma chave boa parecer ruim).
 */
export async function conferirChave(p: VozProvedorAparelho, chave: string, buscar: Buscar): Promise<CategoriaChave | null> {
  if (p === 'xai') return 'invalida'
  const url = p === 'openai' ? 'https://api.openai.com/v1/models' : 'https://generativelanguage.googleapis.com/v1beta/models?pageSize=1'
  const r = await pedir(buscar, url, { method: 'GET', headers: cabecalhos(p, chave) })
  return r.ok ? null : r.categoria
}

/** Credencial de uma sessão de áudio: OpenAI `client_secrets` (10 min) ou token Gemini de um uso. */
export async function cunharCredencial(p: VozProvedorAparelho, chave: string, model: string, voice: string, buscar: Buscar, agora = () => new Date()): Promise<Resultado<string>> {
  if (p === 'xai') return { ok: false, categoria: 'invalida' }
  const headers = { ...cabecalhos(p, chave), 'Content-Type': 'application/json' }
  let r: Resultado<string>
  if (p === 'openai') {
    const body = JSON.stringify({
      expires_after: { anchor: 'created_at', seconds: 600 },
      session: { type: 'realtime', model, audio: { output: { voice } } },
    })
    r = await pedir(buscar, 'https://api.openai.com/v1/realtime/client_secrets', { method: 'POST', headers, body })
  } else {
    const t = agora().getTime()
    const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')
    const body = JSON.stringify({ uses: 1, expireTime: iso(t + 30 * 60_000), newSessionExpireTime: iso(t + 60_000) })
    r = await pedir(buscar, 'https://generativelanguage.googleapis.com/v1alpha/auth_tokens', { method: 'POST', headers, body })
  }
  if (!r.ok) return r
  let valor: unknown
  try { valor = (JSON.parse(r.valor) as Record<string, unknown>)[p === 'openai' ? 'value' : 'name'] } catch { valor = null }
  return typeof valor === 'string' && valor ? { ok: true, valor } : { ok: false, categoria: 'provedor' }
}
