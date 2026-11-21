// Mail Manager: o que a tela mostra a partir da leitura do Hub (`donnaMailManager`) e as regras que ela repete
// para não oferecer o que o Hub recusaria. Quem decide continua sendo o Hub; aqui é só leitura, texto e limite
// de controle. Fail-closed: formato inesperado é erro, nunca lista vazia. Sem React nem I/O.

import type { Mensagem } from '../bot-settings/mensagem.ts'

/** As três permissões independentes de uma caixa, por participante. */
export type PermissaoMail = 'view' | 'send' | 'delete'
/** Quem pode receber acesso a uma caixa. Pessoas dependem de o Hub ter identidade delas. */
export type ParticipanteMail = 'bot' | 'person'

export interface BotMail { botId: string; name: string; isPrimary: boolean }

export interface ConcessaoMail {
  participantKind: ParticipanteMail
  participantId: string
  view: boolean
  send: boolean
  delete: boolean
}

export interface CaixaMail {
  id: string
  label: string
  addresses: string[]
  grants: ConcessaoMail[]
}

export interface LeituraMailManager {
  plugin: { enabled: boolean; mode: 'preview' }
  transportAvailable: boolean
  personGrantsSupported: boolean
  bots: BotMail[]
  mailboxes: CaixaMail[]
}

/** Uma leitura com outro formato é erro, nunca uma lista vazia ou "pronto". */
export function conferirLeitura(raw: unknown): LeituraMailManager {
  const v = raw as LeituraMailManager | null
  const ok = !!v && typeof v === 'object' && !!v.plugin && typeof v.plugin.enabled === 'boolean'
    && v.plugin.mode === 'preview'
    && typeof v.transportAvailable === 'boolean'
    && typeof v.personGrantsSupported === 'boolean'
    && Array.isArray(v.bots)
    && v.bots.every((b) => b && typeof b.botId === 'string' && typeof b.name === 'string' && typeof b.isPrimary === 'boolean')
    && Array.isArray(v.mailboxes)
    && v.mailboxes.every((c) => c && typeof c.id === 'string' && typeof c.label === 'string'
      && Array.isArray(c.addresses) && c.addresses.every((a) => typeof a === 'string')
      && Array.isArray(c.grants) && c.grants.every(concessaoValida))
  if (!ok) throw new Error('resposta_invalida')
  return v
}

function concessaoValida(g: ConcessaoMail): boolean {
  return !!g && (g.participantKind === 'bot' || g.participantKind === 'person')
    && typeof g.participantId === 'string'
    && typeof g.view === 'boolean' && typeof g.send === 'boolean' && typeof g.delete === 'boolean'
}

/** Sem cliente do Hub: a mesma mensagem nas telas e nos estados (guardada como chave, traduzida ao desenhar). */
export const SEM_CONEXAO_MSG: Mensagem = { chave: 'mailManager.erro.semConexao' }

/**
 * Erro do Hub como dado: a chave do catálogo quando o app sabe o que houve; a frase do Hub, como veio, quando
 * ela vem pronta. A tela traduz ao desenhar (`textoDaMensagem`).
 */
export function erroDoHub(error: unknown): Mensagem {
  const raw = (error instanceof Error ? error.message : typeof error === 'object' && error && 'message' in error ? String(error.message) : String(error)).replace(/^Error: /, '')
  if (/unknown method|Not available in the web demo/i.test(raw)) return { chave: 'mailManager.erro.hubAntigo' }
  if (raw === 'resposta_invalida') return { chave: 'mailManager.erro.respostaInvalida' }
  if (/timed? ?out|timeout/i.test(raw)) return { chave: 'mailManager.erro.demorou' }
  const m = /^([a-z_]+): (.+)$/s.exec(raw)
  if (m) {
    const [, codigo, frase] = m
    if (codigo === 'mail_denied') return { chave: 'mailManager.erro.mailDenied' }
    if (codigo === 'mail_invalid') return { chave: 'mailManager.erro.mailInvalid' }
    if (codigo === 'mail_storage_unavailable') return { chave: 'mailManager.erro.armazenamento' }
    if (codigo === 'not_allowed') return { chave: 'mailManager.erro.naoPermitido' }
    return { texto: frase!.charAt(0).toUpperCase() + frase!.slice(1) }
  }
  return { chave: 'mailManager.erro.semFalar' }
}

// MARK: entrada do dono

/** "a@b , c@d;\ne@f" → ["a@b", "c@d", "e@f"]: vírgula, ponto e vírgula ou quebra de linha separam. */
export function parseAddressesInput(raw: string): string[] {
  return raw.split(/[,;\n]/).map((parte) => parte.trim()).filter((parte) => parte.length > 0)
}

// MARK: prévia (domínio reservado, caixas fictícias)

export interface CaixaPrevia { readonly chave: 'suporte' | 'operacoes'; readonly endereco: string }

/** As duas caixas da prévia demonstrativa: fictícias, em domínio reservado. */
export const CAIXAS_PREVIA: readonly CaixaPrevia[] = [
  { chave: 'suporte', endereco: 'suporte@loja.example.invalid' },
  { chave: 'operacoes', endereco: 'operacoes@loja.example.invalid' },
]

/** Quais caixas da prévia ainda não têm o endereço conectado em nenhuma caixa (comparando sem maiúsculas). */
export function caixasPreviaFaltantes(leitura: LeituraMailManager | null): readonly CaixaPrevia[] {
  const conectados = new Set<string>()
  for (const caixa of leitura?.mailboxes ?? []) for (const endereco of caixa.addresses) conectados.add(endereco.toLowerCase())
  return CAIXAS_PREVIA.filter((c) => !conectados.has(c.endereco.toLowerCase()))
}

// MARK: concessões

/** O que este bot pode nesta caixa; sem concessão, nada (fail-closed: tudo falso). */
export function permissoesDoBot(caixa: CaixaMail, botId: string): { view: boolean; send: boolean; delete: boolean } {
  const g = caixa.grants.find((x) => x.participantKind === 'bot' && x.participantId === botId)
  return { view: !!g?.view, send: !!g?.send, delete: !!g?.delete }
}

/** Quantos bots distintos têm ao menos uma permissão ativa na caixa (concessão a pessoa não conta). */
export function contagemAutorizados(caixa: CaixaMail): number {
  const comAcesso = new Set<string>()
  for (const g of caixa.grants) if (g.participantKind === 'bot' && (g.view || g.send || g.delete)) comAcesso.add(g.participantId)
  return comAcesso.size
}
