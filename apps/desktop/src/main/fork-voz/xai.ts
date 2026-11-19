import { pedidoXai, type PedidoXai, type ResultadoCatalogoXai, type VozXai } from '../../shared/fork-voz-xai.ts'
import type { CategoriaChave } from '../../shared/fork-voz.ts'
import { categoriaDaResposta, type Buscar, type Resultado } from './provedor.ts'

const API = 'https://api.x.ai/v1'
async function json(chave: string, buscar: Buscar, caminho: string, body?: unknown): Promise<Resultado<Record<string, unknown>>> {
  try {
    const r = await buscar(`${API}${caminho}`, {
      method: body === undefined ? 'GET' : 'POST', headers: { Authorization: `Bearer ${chave}`, 'Content-Type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(15_000),
    })
    try {
      const texto = await r.text()
      if (r.status < 200 || r.status >= 300) return { ok: false, categoria: categoriaDaResposta(r.status, texto) }
      if (texto.length > 1_048_576) return { ok: false, categoria: 'provedor' }
      const v: unknown = JSON.parse(texto)
      return v && typeof v === 'object' && !Array.isArray(v) ? { ok: true, valor: v as Record<string, unknown> } : { ok: false, categoria: 'provedor' }
    } catch { return { ok: false, categoria: 'provedor' } }
  } catch { return { ok: false, categoria: 'rede' } }
}

function seguro(texto: string, chave: string) {
  if (/[\x00-\x1f\x7f]/.test(texto) || texto.includes(chave)) return false
  for (let i = 0; i <= chave.length - 8; i++) if (texto.includes(chave.slice(i, i + 8))) return false
  return true
}

export async function catalogoXai(chave: string, buscar: Buscar): Promise<ResultadoCatalogoXai> {
  const r = await json(chave, buscar, '/tts/voices')
  if (!r.ok) return r
  if (!Array.isArray(r.valor.voices) || r.valor.voices.length > 1000) return { ok: false, categoria: 'provedor' }
  const vozes = new Map<string, VozXai>()
  for (const v of r.valor.voices) {
    if (!v || typeof v !== 'object' || typeof v.voice_id !== 'string' || !/^[a-z0-9_-]{1,100}$/.test(v.voice_id) ||
      typeof v.name !== 'string' || !v.name.trim() || v.name.length > 150 || !seguro(v.name, chave) || !seguro(v.voice_id, chave) || vozes.has(v.voice_id)) return { ok: false, categoria: 'provedor' }
    vozes.set(v.voice_id, { id: v.voice_id, nome: v.name.trim() })
  }
  return { ok: true, vozes: [...vozes.values()] }
}

export async function conferirXai(chave: string, buscar: Buscar): Promise<CategoriaChave | null> {
  const r = await catalogoXai(chave, buscar)
  return r.ok ? null : r.categoria
}

export async function credencialXai(chave: string, pedido: PedidoXai, buscar: Buscar, agora = Date.now, validar: () => Promise<void> = async () => {}): Promise<Resultado<string>> {
  if (!pedidoXai(pedido)) return { ok: false, categoria: 'invalida' }
  const catalogo = await catalogoXai(chave, buscar)
  if (!catalogo.ok) return catalogo
  if (!catalogo.vozes.some(v => v.id === pedido.voice)) return { ok: false, categoria: 'invalida' }
  await validar()
  // Guia e REST divergem sobre `session`. Não presumir restrição remota ou uso único.
  const r = await json(chave, buscar, '/realtime/client_secrets', { expires_after: { seconds: 300 } })
  if (!r.ok) return r
  const { value, expires_at } = r.valor
  if (typeof value !== 'string' || !/^[A-Za-z0-9._~-]{1,8192}$/.test(value) || !seguro(value, chave) ||
    typeof expires_at !== 'number' || !Number.isInteger(expires_at) || expires_at * 1000 <= agora() || expires_at * 1000 > agora() + 330_000) return { ok: false, categoria: 'provedor' }
  return { ok: true, valor: value }
}
