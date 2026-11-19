// D473 R1/R2 (#548): os painéis da linha de baixo (lista de modelos e menu da mãozinha) presos ao chip.
// A posição é medida DEPOIS do layout (useLayoutEffect) e refeita enquanto o painel está aberto: o tamanho do chip e dos
// contêineres que mudam com a barra lateral (ResizeObserver), resize e scroll da janela. Calcular durante o render
// lia a posição de antes de a barra lateral abrir e soltava o painel (850 → 1100 px: chip em 362, painel em 188).
import { useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'

type Observador = { observe(alvo: Element): void; disconnect(): void }
type Janela = {
  addEventListener(tipo: 'resize' | 'scroll', f: () => void, captura?: boolean): void
  removeEventListener(tipo: 'resize' | 'scroll', f: () => void, captura?: boolean): void
  requestAnimationFrame(f: () => void): number
  cancelAnimationFrame(id: number): void
}
export type Ambiente = { janela: Janela; observador: (aviso: () => void) => Observador }

/** O que, ao mudar de tamanho, pode mover o chip: a coluna do campo e a raiz (a barra lateral abre ou fecha). */
export const conteineresDe = (chip: Element): Element[] =>
  [chip.closest('.composer-dock') ?? chip.parentElement, chip.ownerDocument.documentElement].filter((e): e is Element => !!e)

/**
 * Liga, enquanto o painel está aberto, tudo o que move o chip e chama `reposicionar` no quadro seguinte (o layout já
 * assentou). Devolve o desligar: ao fechar ou desmontar, nada continua ouvindo (sem vazamento).
 */
export function assinarReposicao(chip: Element, conteineres: readonly Element[], reposicionar: () => void, amb: Ambiente): () => void {
  let quadro = 0
  const agendar = () => { amb.janela.cancelAnimationFrame(quadro); quadro = amb.janela.requestAnimationFrame(reposicionar) }
  const observador = amb.observador(agendar)
  observador.observe(chip)
  for (const c of conteineres) observador.observe(c)
  amb.janela.addEventListener('resize', agendar)
  amb.janela.addEventListener('scroll', agendar, true)
  return () => {
    amb.janela.cancelAnimationFrame(quadro)
    observador.disconnect()
    amb.janela.removeEventListener('resize', agendar)
    amb.janela.removeEventListener('scroll', agendar, true)
  }
}

/**
 * Mesma posição (as mesmas chaves com os mesmos valores, rasos): a nova medida não troca o estado. Assim rolar dentro do
 * painel ou da conversa (scroll em captura) não renderiza o seletor de novo a cada quadro (Kodus, #548).
 */
export function mesmaPosicao(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true
  if (!a || !b || typeof a !== 'object' || typeof b !== 'object') return false
  const ka = Object.keys(a), kb = Object.keys(b)
  return ka.length === kb.length && ka.every((k) => Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]))
}

const ambienteReal = (): Ambiente => ({ janela: window, observador: (aviso) => new ResizeObserver(aviso) })

/**
 * A posição do painel presa ao chip: null enquanto fechado (ou antes da primeira medida, que acontece antes da pintura).
 * `calcular` lê o chip já com o layout atual; a assinatura liga ao abrir e desliga ao fechar ou desmontar.
 */
export function usePresaAoChip<T>(aberto: boolean, chip: RefObject<HTMLElement | null>, calcular: (chip: HTMLElement) => T): T | null {
  const [posicao, setPosicao] = useState<T | null>(null)
  const atual = useRef(calcular)
  atual.current = calcular
  useLayoutEffect(() => {
    const el = chip.current
    if (!aberto || !el) { setPosicao(null); return }
    const medir = () => { const nova = atual.current(el); setPosicao((antes) => (mesmaPosicao(antes, nova) ? antes : nova)) }
    medir()
    return assinarReposicao(el, conteineresDe(el), medir, ambienteReal())
  }, [aberto, chip])
  return aberto ? posicao : null
}

/**
 * O painel vai para o <body>: com papel de parede, o vidro do campo tem `backdrop-filter`, que vira o bloco de contenção
 * de um filho `position: fixed` (as coordenadas da janela passariam a contar da caixa). No <body>, `fixed` é a janela.
 */
export function noBody(painel: ReactNode, portal: typeof createPortal = createPortal, doc: Pick<Document, 'body'> = document) {
  return portal(painel, doc.body)
}
