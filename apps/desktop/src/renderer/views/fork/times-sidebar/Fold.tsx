// Barra04 (lib/fold.ts): o corpo dobrável da barra lateral (seções, equipes, grupos de Agentes).
// - O estado vale na hora; o corpo anima sozinho a partir de onde está, então um clique no meio da animação inverte.
// - Antes de abrir a primeira vez, o corpo é montado escondido, `FOLD_BATCH` linhas por quadro (cada lote numa transição):
//   em tempo ocioso (fila) ou já, com o ponteiro ou o foco no título (`warmFold`). Abrir antes de montar tudo espera o
//   último lote e só então anima: nenhum rosto aparece pela metade e nenhum quadro carrega a montagem inteira.
// - Depois de montado, recolher só esconde (`hidden`): reabrir é instantâneo.
import { Children, startTransition, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { FOLD_CLIP, foldKeyframes, WarmQueue } from '../../../lib/fold'
import { reduceMotion } from '../../../lib/theme'

type IdleWindow = Window & { requestIdleCallback?: (cb: () => void, o?: { timeout: number }) => number; cancelIdleCallback?: (id: number) => void }
/** Só quando sobra tempo no quadro (ou, no máximo, depois de `timeout`): a montagem de fundo nunca disputa uma animação. */
function whenIdle(run: () => void, timeout = 4000): () => void {
  const w = window as IdleWindow
  if (w.requestIdleCallback) { const id = w.requestIdleCallback(run, { timeout }); return () => w.cancelIdleCallback?.(id) }
  const id = setTimeout(run, 200)
  return () => clearTimeout(id)
}
const queue = new WarmQueue((run) => { whenIdle(run) })

/** Corpos ainda montando que o ponteiro ou o foco podem apressar. */
const hurry = new Map<string, () => void>()
/** Ponteiro ou foco no título: monta já o corpo dele (um lote por quadro), se ainda não montou. */
export const warmFold = (key: string) => { queue.drop(key); hurry.get(key)?.() }
/** Linhas montadas por quadro antes de abrir: ~3 rostos, poucos ms mesmo numa máquina lenta. */
export const FOLD_BATCH = 3

export function Fold({ open, foldKey, className, id, role, children }: {
  open: boolean
  foldKey: string
  className: string
  id?: string
  role?: string
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  const items = Children.toArray(children)
  // Quantas linhas já montaram (escondidas); Infinity depois da primeira montagem completa.
  const [count, setCount] = useState(open ? Infinity : 0)
  // Montagem: parada, 'idle' (de fundo, só no tempo ocioso) ou 'now' (ponteiro, foco ou pedido de abrir: um lote por quadro).
  const [warming, setWarming] = useState<false | 'idle' | 'now'>(open ? 'now' : false)
  const warm = count >= items.length
  const animation = useRef<{ run: Animation; opening: boolean } | null>(null)
  const first = useRef(true)

  // Recolhido e ainda vazio: entra na fila de montagem ociosa. Pedido de abrir antes disso monta já, em transição.
  const done = count === Infinity
  // Enquanto não montou tudo, o ponteiro ou o foco no título apressam (warmFold).
  useEffect(() => {
    if (done) return
    hurry.set(foldKey, () => setWarming('now'))
    return () => { hurry.delete(foldKey) }
  }, [foldKey, done])
  // Recolhido e vazio: entra na fila de fundo. Pedido de abrir (também no meio da montagem de fundo) apressa.
  useEffect(() => {
    if (done) return
    if (open) { if (warming !== 'now') setWarming('now'); return }
    if (warming) return
    queue.add(foldKey, () => setWarming((w) => w || 'idle'))
    return () => queue.drop(foldKey)
  }, [warming, open, foldKey, done])
  // Um lote por vez, em transição (o React pode fatiar e adiar); completo, passa a montar tudo o que vier.
  useEffect(() => {
    if (!warming || count === Infinity) return
    const step = () => startTransition(() => setCount((c) => (c + FOLD_BATCH >= items.length ? Infinity : c + FOLD_BATCH)))
    if (warming === 'idle') return whenIdle(step)
    const frame = requestAnimationFrame(step)
    return () => cancelAnimationFrame(frame)
  }, [warming, count, items.length])

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const shown = open && warm
    if (first.current) {
      first.current = false
      el.hidden = !shown
      return
    }
    // Para onde o corpo vai agora: o destino da animação em andamento, ou o que está na tela.
    const running = animation.current
    const heading = running ? running.opening : !el.hidden
    if (heading === shown) return
    if (reduceMotion() || typeof el.animate !== 'function') {
      running?.run.cancel()
      animation.current = null
      el.style.clipPath = ''
      el.hidden = !shown
      return
    }
    // Onde o corpo está agora: no meio da outra animação, aberto inteiro ou fechado.
    const box = el.getBoundingClientRect().height
    const opacity = running ? Number(getComputedStyle(el).opacity) : shown ? 0 : 1
    running?.run.cancel()
    el.hidden = false
    const full = el.getBoundingClientRect().height
    const from = running ? { height: box, opacity } : shown ? { height: 0, opacity: 0 } : { height: full, opacity: 1 }
    const { keyframes, options } = foldKeyframes(from, full, shown)
    el.style.clipPath = FOLD_CLIP
    const next = el.animate(keyframes, options)
    animation.current = { run: next, opening: shown }
    next.onfinish = () => {
      if (animation.current?.run !== next) return
      animation.current = null
      el.style.clipPath = ''
      if (!shown) el.hidden = true
    }
  }, [open, warm])

  return <div ref={ref} id={id} role={role} className={className} data-fold={foldKey} data-mounted={done ? undefined : 'parcial'}>{count === Infinity ? items : items.slice(0, count)}</div>
}
