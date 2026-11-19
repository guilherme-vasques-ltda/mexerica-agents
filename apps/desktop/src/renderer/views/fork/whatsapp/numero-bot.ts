// D374 (dono, 06/11/2026): "oculte o número aqui, deixe ele embaçado, só mostre se a pessoa clicar num botão ao lado,
// tipo um olhinho. Isso evita a Meta ver algum print e querer banir o número." O número do bot aparece mascarado
// (nunca só desfocado: o número real fica fora do DOM, do texto copiável e do leitor de tela) e o olhinho mostra por
// 10 s, até trocar de tela/aba ou a janela perder o foco. Nada fica salvo como "sempre visível". Puro, para testar.

/** Depois disto, o número revelado volta a esconder sozinho. */
export const ESCONDER_APOS_MS = 10_000

const digitos = (phone: string) => phone.replace(/\D/g, '')

/** Os 2 últimos dígitos, a única parte que aparece com o número escondido. */
export const finalDoTelefone = (phone: string) => digitos(phone).slice(-2)

/** "+5511900000150" → "+55 •• ••••-••50"; fora do Brasil, "+•• •••• ••50". */
export function mascararTelefone(phone: string): string {
  const fim = finalDoTelefone(phone)
  return digitos(phone).startsWith('55') ? `+55 •• ••••-••${fim}` : `+•• •••• ••${fim}`
}


/**
 * A revelação (D374): `pedido` = o dono clicou no olhinho; `ativo` = a superfície está à vista. Só mostra com os
 * dois. Sair de vista (Expandir, Recolher, painel fechado, troca de tela) cancela o pedido: ao voltar, escondido.
 */
export type Revelacao = { pedido: boolean }
export type EventoRevelacao = { tipo: 'olhinho' } | { tipo: 'esconder' } | { tipo: 'superficie'; ativo: boolean }
export function revelar(r: Revelacao, e: EventoRevelacao): Revelacao {
  if (e.tipo === 'olhinho') return { pedido: !r.pedido }
  if (e.tipo === 'superficie' && e.ativo) return r
  return { pedido: false }
}
export const numeroAMostra = (r: Revelacao, ativo: boolean) => r.pedido && ativo
/** D387: à vista de fato = o que a tela diz (`ativo`, sem valor = sim) E a superfície em volta E com olhinho. */
export const numeroAtivo = (dado: boolean | undefined, aVista: boolean, olho: boolean) => (dado ?? true) && aVista && olho

interface Alvo { addEventListener(tipo: string, fn: () => void): void; removeEventListener(tipo: string, fn: () => void): void }
interface Relogio { setTimeout(fn: () => void, ms: number): unknown; clearTimeout(id: unknown): void }

/**
 * Enquanto o número está à mostra: esconde aos 10 s, quando a janela perde o foco e quando a página fica oculta.
 * Devolve a limpeza (desmontar = trocar de tela/aba também esconde, porque o estado não sobrevive).
 */
export function vigiarRevelado(esconder: () => void, janela: Alvo, doc: Alvo & { visibilityState?: string }, relogio: Relogio = globalThis as unknown as Relogio): () => void {
  const timer = relogio.setTimeout(esconder, ESCONDER_APOS_MS)
  const oculta = () => { if (doc.visibilityState === 'hidden') esconder() }
  janela.addEventListener('blur', esconder)
  doc.addEventListener('visibilitychange', oculta)
  return () => {
    relogio.clearTimeout(timer)
    janela.removeEventListener('blur', esconder)
    doc.removeEventListener('visibilitychange', oculta)
  }
}
