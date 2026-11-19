// MX-93: a abertura sorteada (Pingo ou Sesta) vem pintada no index.html, antes do bundle. Ela não atrasa o app: a
// primeira tela monta por baixo e a abertura sai do DOM sem fade (medido no Electron, o fade de 180 ms atrasava a
// primeira pintura em ~30–50 ms; o fundo é o mesmo --canvas).
// Decisão de UX do dono (16/11/2026): a abertura fica visível por pelo menos MIN_SPLASH_MS, mesmo que o app esteja
// pronto antes (com movimento reduzido, a cena estática fica pelo mesmo tempo). Isso só vale para o que se VÊ: o tempo
// real de pronto (meta MEX-1209, "abrir em até 700 ms") segue medido à parte, na marca MARCA_PRIMEIRA_TELA.

/** Marcas de desempenho da primeira tela (MX-94: régua de abertura): montada (commit) e pintada (quadro seguinte). */
export const MARCA_PRIMEIRA_TELA = 'mexerica:primeira-tela'
export const MARCA_PRIMEIRA_TELA_PINTADA = 'mexerica:primeira-tela-pintada'

export const MARCA_ABERTURA_REMOVIDA = 'mexerica:abertura-removida'

/** Tempo mínimo, em ms desde o início da navegação, em que a abertura fica na tela. */
export const MIN_SPLASH_MS = 2000

/** Sai da hora (sem esperar o mínimo): é o caminho do erro de carga, que nunca fica preso atrás da abertura. */
export function sairDaAbertura(doc: Document = document) {
  doc.getElementById('mx-abertura')?.remove()
}

export interface RelogioAbertura {
  /** Ms desde o início da navegação (0 = abertura pintada); em produção, `performance.now()`. */
  now: () => number
  setTimer: (fn: () => void, ms: number) => unknown
  clearTimer: (id: unknown) => void
  /** Tira a abertura do DOM. */
  remover: () => void
  /** Registra o tempo real de pronto e o tempo exibido (ms), separados. */
  registrar?: (prontoMs: number, exibidoMs: number) => void
}

/** Quanto falta para a abertura completar o mínimo; 0 se o app demorou mais que ele. */
export function esperaRestanteMs(prontoMs: number, minimoMs = MIN_SPLASH_MS) {
  return Math.max(0, minimoMs - prontoMs)
}

/**
 * Controla a saída da abertura com relógio injetável. `pronta()`: a primeira tela montou; espera só o que falta para o
 * mínimo (nada, se já passou). `erro()`: sai agora e cancela a espera. Ambas são idempotentes.
 */
export function criarSaidaDaAbertura(r: RelogioAbertura, minimoMs = MIN_SPLASH_MS) {
  let timer: unknown
  let encerrada = false
  const sair = (prontoMs: number) => {
    if (encerrada) return
    encerrada = true
    r.remover()
    r.registrar?.(prontoMs, r.now())
  }
  return {
    pronta() {
      if (encerrada || timer !== undefined) return
      const prontoMs = r.now()
      const falta = esperaRestanteMs(prontoMs, minimoMs)
      if (falta === 0) return sair(prontoMs)
      timer = r.setTimer(() => sair(prontoMs), falta)
    },
    erro() {
      if (timer !== undefined) r.clearTimer(timer)
      timer = undefined
      sair(r.now())
    },
  }
}

const saida = criarSaidaDaAbertura({
  now: () => performance.now(),
  setTimer: (fn, ms) => setTimeout(fn, ms),
  clearTimer: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
  remover: () => sairDaAbertura(),
  registrar: (prontoMs, exibidoMs) => performance.mark?.(MARCA_ABERTURA_REMOVIDA, { detail: { prontoMs, exibidoMs } }),
})

/** Chamada no commit da primeira tela (useLayoutEffect): marca o tempo real e tira a abertura quando o mínimo passar. */
export function primeiraTelaMontada() {
  performance.mark?.(MARCA_PRIMEIRA_TELA)
  requestAnimationFrame(() => setTimeout(() => performance.mark?.(MARCA_PRIMEIRA_TELA_PINTADA), 0))
  saida.pronta()
}
