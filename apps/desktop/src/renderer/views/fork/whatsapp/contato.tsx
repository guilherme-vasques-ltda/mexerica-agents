import { NumeroBot } from './NumeroBot'
import { useT } from '../../../lib/fork-i18n/estado'
import { nomeDa, pareceTelefone, remetente, telefoneDaConversa, type ConversaWa } from './caixa'
export { nomeEscondido, rotuloConversa, rotuloPrevia } from './contato-rotulos'

// D387 (dono, pergunta 39, A): contatos e grupos sem nome salvo mostram o número escondido com o olhinho, pelo mesmo
// componente do número do bot (NumeroBot, D374), e a conta de saída nos cartões também. O número real nunca vai para
// texto acessível nem para o que se copia com ele escondido.

/** O nome da conversa; sem nome salvo, o número escondido. */
export function NomeConversa({ c, ativo, olho }: { c: Pick<ConversaWa, 'name' | 'phone' | 'kind'>; ativo?: boolean; olho?: boolean }) {
  const tel = telefoneDaConversa(c)
  const t = useT()
  return tel ? <NumeroBot phone={tel} ativo={ativo} olho={olho} /> : <bdi dir="auto">{nomeDa(c, t)}</bdi>
}

/** Remetente em grupo: "~Nome" (não salvo) ou o nome; quando o WhatsApp só dá o número, "~" + número escondido. */
export function Remetente({ name, phone, contact, ativo, olho }: { name: string; phone?: string | null; contact: boolean; ativo?: boolean; olho?: boolean }) {
  if (!contact && pareceTelefone(name)) return <>~<NumeroBot phone={phone || name} ativo={ativo} olho={olho} /></>
  return <bdi dir="auto">{remetente(name, contact)}</bdi>
}

/** A prévia da linha (dentro do botão da linha: só a máscara). */
export function Previa({ c }: { c: ConversaWa }) {
  // R1-01 do #587: o corpo é de terceiros (direção do próprio texto); o remetente também.
  if (c.kind !== 'group') return <bdi dir="auto">{c.last.body}</bdi>
  return <><Remetente name={c.last.senderName} contact={c.last.senderContact} olho={false} />: <bdi dir="auto">{c.last.body}</bdi></>
}

/** A conta de saída de um cartão: com forma de telefone (WhatsApp), escondida; e-mail ou login, como veio. */
export function IdentidadeConta({ identidade }: { identidade: string }) {
  return pareceTelefone(identidade) ? <NumeroBot phone={identidade} /> : <bdi dir="ltr">{identidade}</bdi>
}
