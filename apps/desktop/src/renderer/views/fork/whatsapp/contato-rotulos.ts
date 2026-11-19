import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import { nomeDa, pareceTelefone, remetente, telefoneDaConversa, type ConversaWa } from './caixa.ts'
import { finalDoTelefone, mascararTelefone } from './numero-bot.ts'

// D387: os nomes acessíveis da lista e da conversa, sem o número de quem não tem nome salvo. Puro, para testar.

const escondido = (t: Tradutor, phone: string) => t('whatsapp.numero.escondido', { fim: finalDoTelefone(phone) })

/** O nome da conversa para o leitor de tela, sem o número. */
export function rotuloConversa(t: Tradutor, c: Pick<ConversaWa, 'name' | 'phone' | 'kind'>): string {
  const tel = telefoneDaConversa(c)
  return tel ? escondido(t, tel) : nomeDa(c, t)
}

/** A prévia para o leitor de tela, sem o número do remetente. */
export function rotuloPrevia(t: Tradutor, c: ConversaWa): string {
  if (c.kind !== 'group') return c.last.body
  const quem = !c.last.senderContact && pareceTelefone(c.last.senderName) ? escondido(t, c.last.senderName) : remetente(c.last.senderName, c.last.senderContact)
  return `${quem}: ${c.last.body}`
}


/** O nome da conversa dentro de um texto (ex.: a confirmação de "Apagar esta conversa"): sem nome salvo, a máscara. */
export function nomeEscondido(t: Tradutor, c: Pick<ConversaWa, 'name' | 'phone' | 'kind'>): string {
  const tel = telefoneDaConversa(c)
  return tel ? mascararTelefone(tel) : nomeDa(c, t)
}
