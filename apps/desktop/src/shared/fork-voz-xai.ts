import type { CategoriaChave } from './fork-voz.ts'
import { OPERATOR_TOOLS } from './fork-voz-operador.ts'

// voz-10: interno até a prova paga. Não acrescentar xAI à lista pública de provedores.
export const MODELO_XAI = 'grok-voice-3.0' as const
export const XAI_WS = `wss://api.x.ai/v1/realtime?model=${MODELO_XAI}`
export interface SelecaoXai { model: typeof MODELO_XAI; voice: string; language: string }
export interface PedidoXai extends SelecaoXai { instructions: string }
export interface VozXai { id: string; nome: string }
export type ResultadoCatalogoXai = { ok: true; vozes: VozXai[] } | { ok: false; categoria: CategoriaChave }
export type ResultadoCredencialXai = { ok: true; credential: string; negociacao: string } | { ok: false; categoria: CategoriaChave }
export type ResultadoSessaoXai = { ok: true } | { ok: false; categoria: CategoriaChave }

/** IDs são dados do provedor: não conservar eco reconhecível de credenciais. */
export const idSessaoXai = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9_-]{1,256}$/.test(v) &&
  !/^(?:xai-|sk-|ek_)/i.test(v)

export const idiomaXai = (idioma: string, sistema: string) => (idioma || sistema).replaceAll('_', '-').toLowerCase()
export function pedidoXai(valor: unknown): valor is PedidoXai {
  const p = registro(valor)
  return !!p && Object.keys(p).sort().join(',') === 'instructions,language,model,voice' && p.model === MODELO_XAI &&
    typeof p.voice === 'string' && /^[a-z0-9_-]{1,100}$/.test(p.voice) &&
    typeof p.language === 'string' && /^[a-z]{2,3}(?:-[a-z]{2})?$/.test(p.language) &&
    typeof p.instructions === 'string' && p.instructions.length > 0 && p.instructions.length <= 16_000
}

export function sessaoXai(p: PedidoXai) {
  return {
    model: p.model, voice: p.voice, instructions: p.instructions,
    turn_detection: { type: 'server_vad', idle_timeout_ms: null },
    resumption: { enabled: false },
    audio: {
      input: { format: { type: 'audio/pcm', rate: 24000 }, transport: 'json', transcription: { language_hint: p.language } },
      output: { format: { type: 'audio/pcm', rate: 24000 }, transport: 'json' },
    },
    tools: OPERATOR_TOOLS.map(t => ({ type: 'function', name: t.name, description: t.description, parameters: { ...t.parameters, additionalProperties: false } })),
  }
}

function registro(v: unknown): Record<string, unknown> | null {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? v as Record<string, unknown> : null
}

// A profundidade vem do contrato esperado, nunca do payload não confiável.
function igual(a: unknown, e: unknown): boolean {
  if (Array.isArray(e)) return Array.isArray(a) && a.length === e.length && e.every((v, i) => igual(a[i], v))
  const esperado = registro(e)
  if (!esperado) return a === e
  const atual = registro(a)
  return !!atual && Object.keys(atual).length === Object.keys(esperado).length &&
    Object.keys(esperado).every(k => Object.hasOwn(atual, k) && igual(atual[k], esperado[k]))
}

/** Conferência conservadora em ambos os processos. Campo necessário ausente não é confirmação. */
export function confereSessaoXai(v: unknown, p: PedidoXai): boolean {
  const s = registro(v), e = sessaoXai(p)
  if (!s || s.model !== e.model || s.voice !== e.voice || s.instructions !== e.instructions ||
    !igual(s.turn_detection, e.turn_detection) || !igual(s.resumption, e.resumption) || !igual(s.audio, e.audio) ||
    !Array.isArray(s.tools) || s.tools.length !== e.tools.length) return false
  return e.tools.every(t => {
    const itens = (s.tools as unknown[]).filter(v => registro(v)?.name === t.name)
    return itens.length === 1 && igual(itens[0], t)
  })
}

/** Uma tentativa por gatilho. Instruções completas preservam autoridade e D98 no override da xAI. */
export const iniciarEsperaXai = (instructions: string) => ({
  type: 'response.create', response: {
    instructions: `${instructions}\nNeste turno interno, chame somente bot_updates para aguardar o bot, sem saudação.`,
    metadata: { vozOrigem: 'ferramenta' },
  },
})
