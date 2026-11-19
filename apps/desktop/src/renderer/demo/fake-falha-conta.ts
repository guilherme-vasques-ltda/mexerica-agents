// `?falhaconta=1`: o Hub novo gravando `reason` nas falhas de conta/motor (contrato em host/src/agent/bot/fork_admission.rs).
// Berga (principal) usa a conta Claude Code "Empresa", desconectada; Byte não tem conta e o Codex do Hub não está pronto.
// Tudo fictício: rótulos de exemplo, nenhum token nem e-mail.
import type { ManagedAccount } from '@shared/fork-engine-accounts'

const param = (nome: string) => typeof location !== 'undefined' && new URLSearchParams(location.search).get(nome) === '1'
/** `?cota=1` (MEX-1294/F6-A): as mesmas contas, todas conectadas, com a leitura de uso por conta; sem as falhas de preparo. */
export const cotaContaDemo = () => param('cota')
export const falhaContaDemo = () => param('falhaconta') || cotaContaDemo()

const id = (c: string) => `ac_${c.repeat(64)}`
export const CONTA_EMPRESA = id('1')
export const BOT_COM_CONTA = 'sprint'
export const BOT_SEM_CONTA = 'byte'
/** Crivo usa a conta Reserva (conectada), mas ela não combina com o motor dele: o botão leva à aba Motor. */
export const BOT_TROCA = 'crivo'
export const CONTA_RESERVA = id('3')
export const CONTA_PESSOAL = id('2')

const conta = (idConta: string, label: string, backend: 'claude' | 'codex', state: 'connected' | 'disconnected'): ManagedAccount =>
  ({ id: idConta, label, brand: backend === 'claude' ? 'Anthropic' : 'OpenAI', backend, billing: 'subscription', state, models: [], hiddenModelIds: [], revision: 1, origin: 'managed', botIds: [] })

export const CONTA_SEMANAL = id('4')
export const contasFalhaDemo = () => ({ accounts: [
  conta(CONTA_EMPRESA, 'Empresa', 'claude', cotaContaDemo() ? 'connected' : 'disconnected'),
  conta(CONTA_PESSOAL, 'Pessoal', 'codex', 'connected'),
  conta(CONTA_RESERVA, 'Reserva', 'claude', 'connected'),
  ...(cotaContaDemo() ? [conta(CONTA_SEMANAL, 'Codex só semanal', 'codex', 'connected')] : []),
] })

/**
 * `donnaEngineAccountUsage` fictício, no formato do Hub (host/src/fork/engine_setup/mod.rs): janelas `session`/`week`
 * com `nearLimit` decidido pelo Hub (>= 90%). Empresa está perto do limite na sessão; a conta só semanal tem só a semana
 * (Codex sem janela de sessão); Reserva não tem leitura comprovada (`unavailable`, nunca 0%).
 */
export function usoCotaDemo(engineAccountId: string, agora = Date.now()) {
  const h = 3_600_000
  const janela = (id: 'session' | 'week', percent: number, em: number) => ({ id, percent, resetsAt: agora + em, resetsText: null, nearLimit: percent >= 90 })
  const janelas: Record<string, ReturnType<typeof janela>[]> = {
    [CONTA_EMPRESA]: [janela('session', 92, 2.4 * h), janela('week', 41, 70 * h)],
    [CONTA_PESSOAL]: [janela('session', 62, 1.6 * h), janela('week', 35, 120 * h)],
    [CONTA_SEMANAL]: [janela('week', 95, 30 * h)],
  }
  const windows = janelas[engineAccountId] ?? []
  return { engineAccountId, spaceId: 'pessoal', state: windows.length ? 'available' : 'unavailable', source: windows.length ? 'cli' : null, updatedAt: windows.length ? agora : null, windows }
}

/** O que o Hub grava: o texto fixo (clientes antigos) + `reason` no Notice e na mensagem `failed`. */
const TEXTO_FIXO = 'Não foi possível preparar o pedido após três tentativas. Confira o erro e reenvie o pedido.'
export const entradasFalhaConta = () => [
  { botId: BOT_COM_CONTA, id: 'falha-1-msg', at: 6, kind: 'user' as const, data: { text: 'Fecha o relatório da semana', status: 'failed', error: TEXTO_FIXO, reason: { code: 'account_unavailable', accountId: CONTA_EMPRESA, backend: 'claude' } } },
  { botId: BOT_COM_CONTA, id: 'falha-1-aviso', at: 5, kind: 'notice' as const, data: { text: TEXTO_FIXO, style: 'error', reason: { code: 'account_unavailable', accountId: CONTA_EMPRESA, backend: 'claude' } } },
  { botId: BOT_SEM_CONTA, id: 'falha-2-msg', at: 6, kind: 'user' as const, data: { text: 'Roda os testes', status: 'failed', error: TEXTO_FIXO, reason: { code: 'engine_not_ready', backend: 'codex' } } },
  { botId: BOT_SEM_CONTA, id: 'falha-2-aviso', at: 5, kind: 'notice' as const, data: { text: TEXTO_FIXO, style: 'error', reason: { code: 'engine_not_ready', backend: 'codex' } } },
  { botId: BOT_TROCA, id: 'falha-3-msg', at: 6, kind: 'user' as const, data: { text: 'Revisa o PR', status: 'failed', error: TEXTO_FIXO, reason: { code: 'account_incompatible', accountId: CONTA_RESERVA, backend: 'claude' } } },
  { botId: BOT_TROCA, id: 'falha-3-aviso', at: 5, kind: 'notice' as const, data: { text: TEXTO_FIXO, style: 'error', reason: { code: 'account_incompatible', accountId: CONTA_RESERVA, backend: 'claude' } } },
]

/**
 * `donnaEngineFallback*` fictício (MEX-1391/F6-B), no formato do Hub (host/src/fork/fallback_contas/api.rs). No cenário
 * `?cota=1`: Empresa (92%) descansa por "perto do limite", a conversa do Berga roda na Reserva e a ordem do Claude já tem a
 * Reserva. Salvar e as ações mudam só este estado em memória; o Hub real confere tudo de novo.
 */
const estadoReserva = (agora: number) => ({
  spaceId: 'pessoal',
  orders: [{ backend: 'claude', accounts: [CONTA_RESERVA] }] as { backend: string; accounts: string[] }[],
  botOrders: [] as unknown[],
  resting: [{ engineAccountId: CONTA_EMPRESA, restUntil: agora + 2.4 * 3_600_000, reason: 'perto_do_limite', extraUntil: null as number | null, blockedUntil: null }],
  sessions: [{ sessionId: BOT_COM_CONTA, botId: BOT_COM_CONTA, originAccountId: CONTA_EMPRESA, currentAccountId: CONTA_RESERVA, since: agora - 600_000, reason: 'perto_do_limite', originBackAt: agora + 2.4 * 3_600_000 }],
})
let reservaDemo: ReturnType<typeof estadoReserva> | null = null
export function fallbackDemo(method: string, body: Record<string, unknown>, agora = Date.now()) {
  reservaDemo ??= estadoReserva(agora)
  if (method === 'donnaEngineFallbackSave') {
    const backend = String(body.backend)
    reservaDemo.orders = [...reservaDemo.orders.filter((o) => o.backend !== backend), { backend, accounts: (body.accounts as string[] | null) ?? [] }].filter((o) => o.accounts.length)
  } else if (method === 'donnaEngineFallbackAction') {
    const alvo = reservaDemo.resting.find((r) => r.engineAccountId === body.engineAccountId)
    if (body.action === 'backNow') reservaDemo.resting = reservaDemo.resting.filter((r) => r !== alvo)
    else if (alvo) alvo.extraUntil = alvo.restUntil
  }
  return structuredClone(reservaDemo)
}
