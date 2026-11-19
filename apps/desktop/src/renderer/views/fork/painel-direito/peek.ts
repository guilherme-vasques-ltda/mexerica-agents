// Painel direito solto (Barra05, passo 15): abre 90 ms depois do ponteiro entrar na faixa de 18 px da borda e
// fecha 280 ms depois de sair do painel. Fixo, não depende do ponteiro. Puro e com relógio injetável para testar.

export const PEEK_OPEN_MS = 90
export const PEEK_CLOSE_MS = 280

type Timers = { set: (fn: () => void, ms: number) => unknown; clear: (id: unknown) => void }
const realTimers: Timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id as ReturnType<typeof setTimeout>) }

export class PanelPeek {
  /** Aberto por cima do chat (só quando solto). */
  open = false
  private timer: unknown = null
  private readonly onChange: (open: boolean) => void
  private readonly timers: Timers

  constructor(onChange: (open: boolean) => void, timers: Timers = realTimers) {
    this.onChange = onChange
    this.timers = timers
  }

  /** O ponteiro entrou na faixa da borda. */
  edgeEnter() { this.schedule(true, PEEK_OPEN_MS) }
  /** O ponteiro saiu da faixa sem entrar no painel. */
  edgeLeave() { this.schedule(false, PEEK_CLOSE_MS) }
  /** O ponteiro está no painel: nada fecha. */
  panelEnter() { this.cancel() }
  /** Saiu do painel; um campo dele com foco o mantém aberto. */
  panelLeave(editing = false) { if (!editing) this.schedule(false, PEEK_CLOSE_MS) }
  /** "···", um atalho da barra ou um pedido do Hub: abre já. */
  show() { this.cancel(); this.set(true) }
  /** Esc, clique fora ou fixar: fecha já. */
  dismiss() { this.cancel(); this.set(false) }
  dispose() { this.cancel() }

  private schedule(open: boolean, ms: number) {
    this.cancel()
    if (open === this.open) return
    this.timer = this.timers.set(() => { this.timer = null; this.set(open) }, ms)
  }

  private cancel() {
    if (this.timer !== null) this.timers.clear(this.timer)
    this.timer = null
  }

  private set(open: boolean) {
    if (open === this.open) return
    this.open = open
    this.onChange(open)
  }
}
