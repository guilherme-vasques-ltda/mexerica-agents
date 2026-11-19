// Pedidos de outros bots na bandeja "Na fila" (entrega do dono 10/11/2026): a cena `?pedidos=N` põe o Crivo
// respondendo com N pedidos de bots na fila da conversa principal dele — avisos `notice` com `delegationId`,
// `botMessage` e a lane do remetente (`sourceChat`/`sourceSession`, gravados pelo Hub na admissão). As três ações
// seguem o formato do Hub: a mesma RPC `queuedRemove`/`queuedEdit`/`queuedSendNow`, agora também para o aviso do
// destinatário (host/src/agent/bot/fork_fila.rs).
import type { Entry } from '@shared/models'

export const BOT_PEDIDOS = 'crivo'

/** Quantos pedidos a cena `?pedidos=N` põe na fila (0 = cena desligada). */
export const pedidosTotal = () => {
  if (typeof location === 'undefined') return 0
  const n = Number(new URLSearchParams(location.search).get('pedidos'))
  return Number.isInteger(n) && n > 0 ? n : 0
}

export const pedidosDemo = () => pedidosTotal() > 0

interface Pedido {
  /** ask_bot espera resposta ("Request from"), message_bot só registra ("Message from"). */
  tipo: 'ask' | 'message'
  sourceBotId: string
  sourceName: string
  /** Lane do remetente: conversa (= botId na principal) e sessão extra, quando houver. */
  sourceChat: string
  sourceSession: string | null
  sessionTitle: string | null
  text: string
}

const PEDIDOS: Pedido[] = [
  {
    tipo: 'ask', sourceBotId: 'byte', sourceName: 'Byte', sourceChat: 'byte', sourceSession: null, sessionTitle: null,
    text: 'Revise o diff do src/pace.js do último commit e diga se o guarda de data inválida cobre o caso de virada de ano.',
  },
  {
    tipo: 'message', sourceBotId: 'prosa', sourceName: 'Prosa', sourceChat: 'prosa', sourceSession: null, sessionTitle: null,
    text: 'Quando terminar a revisão, atualize o rascunho das notas da 1.4 com o resultado do Crivo na seção de qualidade.',
  },
  {
    tipo: 'ask', sourceBotId: 'byte', sourceName: 'Byte', sourceChat: 'byte', sourceSession: 'byte-guard', sessionTitle: 'Invalid-date guard',
    text: 'O benchmark weeklyTotals rodou limpo? Se sim, pode fechar a sessão com um resumo de uma linha para o changelog.',
  },
  {
    tipo: 'message', sourceBotId: 'sprint', sourceName: 'Berga', sourceChat: 'sprint', sourceSession: null, sessionTitle: null,
    text: 'Anote no quadro da Lançamento que a revisão do pace.js depende do guarda de data; se reprovar, devolva para o Byte.',
  },
]

const STATUS_INICIAL: Record<Pedido['tipo'], string> = { ask: 'Waiting for a reply…', message: "Queued. Outcome will appear in Crivo's chat." }

/** O aviso do destinatário como o Hub grava (team.rs): texto, heading, status e botMessage. */
function avisoPedido(p: Pedido, id: string): { kind: string; data: Entry['data'] } {
  const verbo = p.tipo === 'ask' ? 'Request from' : 'Message from'
  const heading = `${verbo} ${p.sourceName}: ${p.text}`
  return {
    kind: 'notice',
    data: {
      text: `${heading}\n${STATUS_INICIAL[p.tipo]}`,
      heading,
      style: 'info',
      status: 'queued',
      delegationId: id,
      sourceBotId: p.sourceBotId,
      targetBotId: BOT_PEDIDOS,
      sourceChat: p.sourceChat,
      sourceSession: p.sourceSession,
      botMessage: { sourceBotId: p.sourceBotId, targetBotId: BOT_PEDIDOS, text: p.text },
    },
  }
}

/** A conversa da cena: o pedido do dono, a resposta em andamento e N pedidos de bots na fila. */
export function entradasPedidos(): { kind: string; data: Entry['data'] }[] {
  const pedidos = PEDIDOS.slice(0, pedidosTotal())
  return [
    { kind: 'user', data: { text: 'Crivo, revisa o diff do pace.js e me diz se pode subir.', status: 'sent' } },
    { kind: 'agent', data: { text: 'Estou lendo o diff do src/pace.js e rodando a suíte. O guarda de data inválida parece cobrir a virada de ano; falta conferir os testes de weeklyTotals…', final: true, author: BOT_PEDIDOS } },
    ...pedidos.map((p, i) => avisoPedido(p, `demo-pedido-${i + 1}`)),
  ]
}

export function atendePedidos(method: string) {
  return pedidosDemo() && (method === 'queuedRemove' || method === 'queuedEdit' || method === 'queuedMove' || method === 'queuedSendNow')
}

const NOME_ALVO = 'Crivo'

/** As mesmas recusas do Hub para o aviso do destinatário: só pedido ainda na fila, nunca o do remetente. */
export function chamarPedidos(method: string, b: Record<string, unknown>, deps: {
  entries: Entry[]
  emit: (e: Entry) => void
  fail: (m: string) => unknown
}) {
  const entry = deps.entries.find((e) => e.id === b.entryId)
  const message = entry?.data.botMessage
  const alvo = entry?.kind === 'notice' && entry.data.status === 'queued' && entry.data.delegationId
    && message?.targetBotId === entry.botId ? entry : undefined
  if (!alvo || !message) throw deps.fail('queued_entry_dispatched: o pedido já saiu da fila')
  if (method === 'queuedEdit' && b.expectedRevision !== alvo.rev) {
    throw deps.fail('queued_revision_conflict: a mensagem mudou; releia a fila')
  }
  const tipo = alvo.data.heading?.startsWith('Request from') ? 'ask' : 'message'
  const origem = alvo.data.sourceBotId ?? ''
  const nome = PEDIDOS.find((p) => p.sourceBotId === origem && p.sourceSession === (alvo.data.sourceSession ?? null))?.sourceName ?? origem
  const cauda = tipo === 'ask' ? 'Waiting for a reply…' : `Queued. Outcome will appear in ${NOME_ALVO}'s chat.`
  // O motivo exato que o Host grava ao recusar o pedido na bandeja (team.rs).
  const motivo = 'Cancelled by the owner before execution.'
  switch (method) {
    case 'queuedRemove':
      alvo.data = {
        ...alvo.data,
        status: 'cancelled',
        text: `${alvo.data.heading}\n${motivo}`,
        style: 'error',
        botMessage: { ...message, detail: motivo },
      }
      deps.emit(alvo)
      break
    case 'queuedEdit': {
      const text = String(b.text ?? '').trim()
      if (!text) throw deps.fail('empty message')
      const heading = `${tipo === 'ask' ? 'Request from' : 'Message from'} ${nome}: ${text}`
      alvo.data = {
        ...alvo.data,
        heading,
        text: `${heading}\n${cauda}`,
        botMessage: { ...message, text, edited: true },
      }
      deps.emit(alvo)
      break
    }
    case 'queuedMove':
      throw deps.fail('queued_move_unsupported: pedidos de bot não mudam de lugar')
    case 'queuedSendNow':
      // Como no Host: o pedido sai de `queued` no despacho — ask mantém o texto, message ganha "Running…".
      alvo.data = tipo === 'ask'
        ? { ...alvo.data, status: 'sent' }
        : { ...alvo.data, status: 'sent', text: `${alvo.data.heading}\nRunning in ${NOME_ALVO}'s chat…` }
      deps.emit(alvo)
      return { entries: [alvo], stopRequested: true }
    default:
      throw deps.fail('unknown queued action')
  }
  return { entries: [alvo], stopRequested: false }
}
