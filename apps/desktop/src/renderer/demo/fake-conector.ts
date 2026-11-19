import type { EntryData } from '@shared/models'

// R6a: cartões de conector sintéticos para os prints, só com `?conector` na URL da demo.
// Formas iguais às do Hub (host/src/fork/conectores/operacao.rs: public/receipt/presentation).
// Nenhuma conta, rede ou WhatsApp: tudo em memória, dados de exemplo.

type Caso = { id: string; state: string; aberto: boolean; protocolo?: number; vencido?: boolean; recusa?: string; apresentacao?: object }

// Conta, rótulos e ação no formato da apresentação do protocolo 2; números e e-mails de exemplo.
const whatsapp = {
  efeito: 'Enviar mensagem no WhatsApp', ferramenta: 'enviar_mensagem',
  argumentos: { para: 'Carlos (exemplo) +55 11 90000-0101', texto: 'Oi, Carlos! Recebi seu pedido para sábado às 9h, para duas pessoas. Vou conferir o horário e te confirmo ainda hoje.' },
  rotulos: { para: 'Destinatário', texto: 'Mensagem' }, conta: { rotulo: 'WhatsApp da Prosa (exemplo)', identidade: '+55 11 90000-0001' },
}
const gmail = {
  efeito: 'Guardar um rascunho no Gmail', ferramenta: 'criar_rascunho',
  argumentos: { para: 'bruno@cliente.exemplo.com', assunto: 'Re: Orçamento do site', corpo: 'Oi, Bruno. Segue a proposta nova com os três ajustes que você pediu.' },
  rotulos: { para: 'Para', assunto: 'Assunto', corpo: 'Mensagem' }, conta: { rotulo: 'Gmail da Prosa (exemplo)', identidade: 'prosa@exemplo.com' },
}

const CASOS: Caso[] = [
  { id: 'aguardando', state: 'aguardando', aberto: true, apresentacao: whatsapp },
  { id: 'gmail', state: 'aguardando', aberto: true, apresentacao: gmail },
  { id: 'sem_conta', state: 'aguardando', aberto: true, apresentacao: { ...whatsapp, conta: null } },
  { id: 'autorizada', state: 'autorizada', aberto: true, apresentacao: whatsapp },
  { id: 'em_envio', state: 'em_envio', aberto: true, apresentacao: whatsapp },
  { id: 'concluida', state: 'concluida', aberto: false },
  { id: 'falha_comprovada', state: 'falha_comprovada', aberto: false },
  { id: 'incerta', state: 'incerta', aberto: false },
  { id: 'recusada', state: 'recusada', aberto: false },
  { id: 'expirada', state: 'expirada', aberto: false },
  { id: 'vencido', state: 'aguardando', aberto: true, vencido: true, apresentacao: whatsapp },
  { id: 'sem_suporte', state: 'aguardando', aberto: true, protocolo: 1 },
  { id: 'sem_acesso', state: 'aguardando', aberto: true, recusa: 'Conectores curados exigem o operador Local; Device/voz/ACS4 não concedem autoridade' },
]

export const conectorDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('conector')

/** Entradas `permission` públicas, como o Hub projeta (sem conteúdo privado). */
export function entradasConector(autor: string): { id: string; data: EntryData }[] {
  return CASOS.map((c) => ({
    id: `conector-${c.id}`,
    data: {
      author: autor, status: c.aberto ? 'pending' : c.state, voiceApproval: 'app_only',
      text: 'Pedido de conector. Confira o recibo no app compatível.',
      conectorOperation: { tentativaId: c.id, cardId: `cartao-${c.id}`, state: c.state, decisionProtocol: (c.protocolo ?? 2) as 2 },
    } as EntryData,
  }))
}

export class FakeConector {
  readonly pedidos: { metodo: string; corpo: Record<string, unknown> }[] = []
  private estados = new Map(CASOS.map((c) => [c.id, c.state]))

  atende(method: string) { return method.startsWith('donnaConector') }

  call(method: string, b: Record<string, unknown>, fail: (m: string) => unknown): unknown {
    this.pedidos.push({ metodo: method, corpo: structuredClone(b) })
    const caso = CASOS.find((c) => c.id === b.tentativaId)
    if (!caso) throw fail('Tentativa não encontrada')
    if (caso.recusa) throw fail(caso.recusa)
    const state = this.estados.get(caso.id)!
    const recibo = {
      tentativaId: caso.id, snapshotId: `snap-${caso.id}`, cardId: `cartao-${caso.id}`, cardRev: 1, requestDigest: '0'.repeat(64),
      state, delivery: 'nao_liberada', t0: Date.now() - 20_000, cardDeadline: caso.vencido ? Date.now() - 1000 : Date.now() + 3_600_000,
      globalDeadline: Date.now() + 3_600_000, admittedAt: null, completedAt: null, decisionProtocol: 2, evidence: 'sem_conclusao_externa',
    }
    switch (method) {
      case 'donnaConectorTentativa':
        return ['aguardando', 'autorizada', 'em_envio'].includes(state) ? { receipt: recibo, presentation: caso.apresentacao ?? null } : recibo
      case 'donnaConectorDecidir':
        this.estados.set(caso.id, b.decisao === 'recusar' ? 'recusada' : 'autorizada')
        return { ...recibo, state: this.estados.get(caso.id) }
      case 'donnaConectorEditar':
        throw fail('Campo não editável')
      default:
        throw fail('Método desconhecido')
    }
  }
}
