// DON-907 (D431), só na demo: a fila de Crivo com N mensagens enquanto ele responde, e as quatro ações no formato
// do Hub (host/src/agent/bot/fork_fila.rs). `?fila=N` liga a cena com N na fila; sem o parâmetro, a demo fica como antes.
import type { Entry } from '@shared/models'

/** Quantas mensagens a cena `?fila=N` põe na fila (0 = cena desligada). */
export const filaTotal = () => {
  if (typeof location === 'undefined') return 0
  const n = Number(new URLSearchParams(location.search).get('fila'))
  return Number.isInteger(n) && n > 0 ? n : 0
}

export const filaDemo = () => filaTotal() > 0

export const BOT_FILA = 'crivo'

const TEXTOS_FILA = [
  'Separe também as assinaturas (Netflix, Spotify, iCloud) numa aba própria.',
  'Coloque a coluna de total por categoria no fim, com a soma do mês.',
  'E me avise se algum gasto passar de R$ 500.',
  'Destaque em amarelo os lançamentos duplicados entre Itaú e Nubank.',
  'Some os gastos com delivery por semana e faça um gráfico de colunas.',
  'Crie uma aba só com os gastos fixos: aluguel, internet e academia.',
  'Compare outubro com setembro e mostre o que subiu mais de 20%.',
  'Marque em vermelho as assinaturas sem uso há mais de 3 meses.',
  'Exporte o resumo por categoria em CSV para eu mandar ao contador.',
  'Separe os gastos do cartão virtual dos do cartão físico.',
  'Faça uma previsão de novembro com a média dos últimos 3 meses.',
  'Deixe a planilha pronta para imprimir em A4, uma categoria por página.',
]

/** A conversa da cena: o pedido, a resposta em andamento e N mensagens na fila. */
export function entradasFila(): { kind: string; data: Entry['data'] }[] {
  const fila: { kind: string; data: Entry['data'] }[] = []
  for (let i = 0; i < filaTotal(); i++) {
    fila.push({ kind: 'user', data: { text: TEXTOS_FILA[i] ?? `Pedido ${i + 1} da planilha de outubro.`, status: 'queued' } })
  }
  return [
    { kind: 'user', data: { text: 'Monte a planilha de gastos de outubro a partir dos extratos da pasta Financeiro.', status: 'sent' } },
    { kind: 'agent', data: { text: 'Comecei pelos extratos do Itaú e do Nubank. Já separei 84 lançamentos em 9 categorias; falta conferir os do cartão…', final: true, author: BOT_FILA } },
    ...fila,
  ]
}

const chave = (e: Entry) => e.data.queueOrder ?? e.seq

export function atendeFila(method: string) {
  return filaDemo() && (method === 'queuedRemove' || method === 'queuedEdit' || method === 'queuedMove' || method === 'queuedSendNow')
}

/** As mesmas recusas do Hub: só mensagem ainda na fila, com a revisão esperada (editar/mover). */
export function chamarFila(method: string, b: Record<string, unknown>, deps: {
  entries: Entry[]
  emit: (e: Entry) => void
  notice: (botId: string, text: string) => void
  working: (botId: string) => boolean
  fail: (m: string) => unknown
}) {
  const entry = deps.entries.find((e) => e.id === b.entryId)
  if (!entry || entry.kind !== 'user' || entry.data.status !== 'queued') throw deps.fail('queued_entry_dispatched: a mensagem já saiu da fila')
  if ((method === 'queuedEdit' || method === 'queuedMove') && b.expectedRevision !== entry.rev) throw deps.fail('queued_revision_conflict: a mensagem mudou')
  const sessao = entry.sessionId ?? entry.botId
  const fila = deps.entries.filter((e) => e.kind === 'user' && e.data.status === 'queued' && e.botId === entry.botId && (e.sessionId ?? e.botId) === sessao)
    .sort((a, c) => chave(a) - chave(c))
  const reordenar = (para: number) => {
    const chaves = fila.map(chave)
    const nova = fila.filter((e) => e !== entry)
    nova.splice(Math.min(para, nova.length), 0, entry)
    nova.forEach((e, i) => { if (chave(e) !== chaves[i]) { e.data = { ...e.data, queueOrder: chaves[i] }; deps.emit(e) } })
  }
  switch (method) {
    case 'queuedRemove':
      entry.data = { ...entry.data, status: 'cancelled' }
      deps.emit(entry)
      break
    case 'queuedEdit':
      entry.data = { ...entry.data, text: String(b.text ?? '').trim() }
      deps.emit(entry)
      break
    case 'queuedMove':
      reordenar(Number(b.position))
      break
    case 'queuedSendNow': {
      reordenar(0)
      const interrompe = deps.working(entry.botId)
      if (interrompe) deps.notice(entry.botId, 'Resposta interrompida para atender a próxima mensagem.')
      entry.data = { ...entry.data, status: 'sent' }
      deps.emit(entry)
      return { entries: [entry], stopRequested: interrompe }
    }
  }
  return { entries: [entry], stopRequested: false }
}
