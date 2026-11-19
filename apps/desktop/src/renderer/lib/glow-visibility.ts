// MX-1287 (D2): o halo do header respira 9 s. Duas pausas, só atributos no <html> (o CSS em
// views/shell/janela-header.css faz o `animation-play-state: paused`; nada de estilo nem I/O aqui):
// - `data-janela-oculta`: documento oculto OU janela sem foco; volta ao focar.
// - `data-janela-ociosa`: janela em primeiro plano, mas 60 s sem mouse, teclado, rolagem ou toque. Com o app parado
//   a respiração é o único movimento da tela e mantém o compositor (processo de GPU) acordado para nada; qualquer
//   interação retoma. Os mesmos atributos pausam os loops ambientes (bot dormindo, Mexeriquinha livre/dormindo).

export const ATRIBUTO_OCULTA = 'data-janela-oculta'
export const ATRIBUTO_OCIOSA = 'data-janela-ociosa'
export const OCIOSIDADE_MS = 60_000
// Entradas que contam como "a pessoa está aqui". Passivos; o custo por evento é uma leitura de relógio.
export const EVENTOS_INTERACAO = ['pointermove', 'pointerdown', 'keydown', 'wheel', 'touchstart'] as const

type Ouvinte = () => void

interface Docs {
  visibilityState: string
  hasFocus?(): boolean
  documentElement: { setAttribute(n: string, v: string): void; removeAttribute(n: string): void }
  addEventListener(t: 'visibilitychange', cb: Ouvinte): void
  removeEventListener(t: 'visibilitychange', cb: Ouvinte): void
}

interface Janela {
  addEventListener(t: 'focus' | 'blur' | (typeof EVENTOS_INTERACAO)[number], cb: Ouvinte, opcoes?: { passive?: boolean; capture?: boolean }): void
  removeEventListener(t: 'focus' | 'blur' | (typeof EVENTOS_INTERACAO)[number], cb: Ouvinte, opcoes?: { capture?: boolean }): void
}

interface Relogio {
  agora(): number
  setTimeout(cb: () => void, ms: number): unknown
  clearTimeout(id: unknown): void
}

const relogioReal: Relogio = {
  agora: () => Date.now(),
  setTimeout: (cb, ms) => setTimeout(cb, ms),
  clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
}

/** Espelha visibilidade, foco e ociosidade em atributos no root; devolve a função que desliga. */
export function acompanharJanelaOculta(doc: Docs = document, janela: Janela = window, relogio: Relogio = relogioReal): () => void {
  const raiz = doc.documentElement
  const aplicar = () => {
    const semFoco = doc.hasFocus ? !doc.hasFocus() : false
    if (doc.visibilityState === 'hidden' || semFoco) raiz.setAttribute(ATRIBUTO_OCULTA, '')
    else raiz.removeAttribute(ATRIBUTO_OCULTA)
  }

  // Ociosidade: um único timer que se rearma pelo tempo que falta (não um clearTimeout por pointermove).
  let ultima = relogio.agora()
  let ociosa = false
  let timer: unknown = null
  const armar = (ms: number) => { timer = relogio.setTimeout(vencer, ms) }
  function vencer() {
    const falta = OCIOSIDADE_MS - (relogio.agora() - ultima)
    if (falta > 0) { armar(falta); return }
    timer = null
    ociosa = true
    raiz.setAttribute(ATRIBUTO_OCIOSA, '')
  }
  const interagiu = () => {
    ultima = relogio.agora()
    if (ociosa) { ociosa = false; raiz.removeAttribute(ATRIBUTO_OCIOSA) }
    if (timer === null) armar(OCIOSIDADE_MS)
  }

  aplicar()
  armar(OCIOSIDADE_MS)
  doc.addEventListener('visibilitychange', aplicar)
  janela.addEventListener('focus', aplicar)
  janela.addEventListener('blur', aplicar)
  // Voltar ao foco também é a pessoa de volta: conta como interação. Captura, para nenhum stopPropagation esconder o evento.
  janela.addEventListener('focus', interagiu)
  for (const t of EVENTOS_INTERACAO) janela.addEventListener(t, interagiu, { passive: true, capture: true })
  return () => {
    doc.removeEventListener('visibilitychange', aplicar)
    janela.removeEventListener('focus', aplicar)
    janela.removeEventListener('blur', aplicar)
    janela.removeEventListener('focus', interagiu)
    for (const t of EVENTOS_INTERACAO) janela.removeEventListener(t, interagiu, { capture: true })
    if (timer !== null) relogio.clearTimeout(timer)
    raiz.removeAttribute(ATRIBUTO_OCULTA)
    raiz.removeAttribute(ATRIBUTO_OCIOSA)
  }
}
