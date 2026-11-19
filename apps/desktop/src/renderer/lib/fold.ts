// Barra04 (dono, 05/11/2026: "leve animação, e não seco"): abrir e fechar com movimento as seções, as equipes e os
// grupos da barra lateral. Abrir: o corpo cresce até a altura com fade (240 ms, curva de saída); fechar: encolhe
// (180 ms, curva de entrada) e some. Um novo pedido no meio inverte a partir de onde o corpo está: vale a última
// intenção. Recorte com `clip-path`, não `overflow`, para as linhas da árvore (que passam da borda) não serem cortadas.
//
// Revisão do #355 (medição no Mac, 05/11/2026): montar ~10 rostos de uma vez custava um quadro de ~34 ms. O corpo
// agora é montado antes de abrir, escondido, em tempo ocioso (`WarmQueue`) e pelo ponteiro ou o foco no título; ao
// fechar, ele fica montado e escondido, e reabrir é instantâneo. Este arquivo tem só regras puras (testadas).

export const FOLD_OPEN = { duration: 240, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' } as const
export const FOLD_CLOSE = { duration: 180, easing: 'cubic-bezier(0.4, 0, 1, 1)' } as const
export const FOLD_CLIP = 'inset(0 -48px 0 -48px)'

/**
 * A animação de um corpo que vai abrir (`opening`) ou fechar, a partir de onde ele está agora (`from`: a altura e a
 * opacidade no meio de outra animação, ou 0/0 se estava fechado, `full`/1 se estava aberto) até a altura inteira
 * (`full`) ou zero. A duração e a curva são as do contrato, inteiras, também numa inversão.
 */
export function foldKeyframes(from: { height: number; opacity: number }, full: number, opening: boolean) {
  const to = opening ? { height: full, opacity: 1 } : { height: 0, opacity: 0 }
  return {
    keyframes: [{ height: `${from.height}px`, opacity: from.opacity }, { height: `${to.height}px`, opacity: to.opacity }],
    options: opening ? FOLD_OPEN : FOLD_CLOSE,
  }
}

/**
 * Fila de corpos a montar antes de abrir. Cada um entra ao aparecer recolhido; a fila monta um por vez, quando o
 * navegador está ocioso (`schedule`, `requestIdleCallback` com prazo no app). `now` adianta um corpo (ponteiro ou
 * foco no título): ele sai da fila e monta já. `drop` tira quem desmontou. Monta cada corpo uma vez só.
 */
export class WarmQueue {
  private waiting = new Map<string, () => void>()
  private scheduled = false
  private readonly schedule: (run: () => void) => void

  constructor(schedule: (run: () => void) => void) { this.schedule = schedule }

  add(key: string, warm: () => void) {
    if (this.waiting.has(key)) return
    this.waiting.set(key, warm)
    this.kick()
  }

  now(key: string) {
    const warm = this.waiting.get(key)
    if (!warm) return
    this.waiting.delete(key)
    warm()
  }

  drop(key: string) { this.waiting.delete(key) }

  get size() { return this.waiting.size }

  private kick() {
    if (this.scheduled || !this.waiting.size) return
    this.scheduled = true
    this.schedule(() => {
      this.scheduled = false
      const next = this.waiting.entries().next()
      if (!next.done) {
        const [key, warm] = next.value
        this.waiting.delete(key)
        warm()
      }
      this.kick()
    })
  }
}
