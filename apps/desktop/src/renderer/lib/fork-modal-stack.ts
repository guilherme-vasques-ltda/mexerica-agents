// Adaptado de Codync 219935410dffccb950f90d0565bd1d9f1661bd5b (Apache-2.0).
// leepokai; coautoria histórica Claude Opus 5.5. Modificado pela DONNA Bot em 2026-11-06:
// gerência única, saída inerte, foco válido e Escape atômico. Ver THIRD_PARTY.md.
import { useLayoutEffect, useRef, type RefObject } from 'react'
import { trapWrap } from './focus-trap.ts'

export interface PageLayer<T> {
  el: T
  z: number
  modal: boolean
  inert: boolean
  muted: boolean
  exiting?: boolean
}

/** A ordem recebida é a ordem DOM; z-index decide primeiro, DOM desempata. */
export function stackModals<T>(layers: readonly PageLayer<T>[]) {
  const top = layers.reduce((best, layer, i) => layer.modal && !layer.exiting && (best < 0 || layer.z >= layers[best]!.z) ? i : best, -1)
  const live = (layer: PageLayer<T>, i: number) => !layer.exiting && (top < 0 || i === top || (!layer.modal && i > top && layer.z >= layers[top]!.z))
  return {
    top: top < 0 ? null : layers[top]!.el,
    mute: layers.filter((layer, i) => !live(layer, i) && !layer.inert).map((layer) => layer.el),
    unmute: layers.filter((layer, i) => live(layer, i) && layer.muted).map((layer) => layer.el),
  }
}

const openModals = new Map<HTMLElement, HTMLElement | null>()
const exiting = new Set<HTMLElement>()
const muted = new Set<HTMLElement>()
const FOCUSABLE = 'button:not(:disabled), [href], input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])'
let top: HTMLElement | null = null
let observer: MutationObserver | null = null

/** Inclui inert/oculto em ancestrais, não apenas no nó que recebe a ref. */
export function interactive(node: HTMLElement | null): node is HTMLElement {
  return !!node?.isConnected && !node.matches(':disabled') && !node.closest('[inert], [hidden], [aria-hidden="true"]')
    && node.getClientRects().length > 0 && getComputedStyle(node).visibility !== 'hidden'
    && ![...exiting].some((layer) => layer.contains(node))
}

function syncModals() {
  const layers = [...document.body.children].filter((el): el is HTMLElement => el instanceof HTMLElement)
  const next = stackModals(layers.map((el) => ({ el, z: Number(getComputedStyle(el).zIndex) || 0, modal: openModals.has(el), inert: el.inert, muted: muted.has(el), exiting: exiting.has(el) })))
  top = next.top
  for (const el of next.mute) { el.inert = true; muted.add(el) }
  for (const el of next.unmute) { el.inert = false; muted.delete(el) }
  for (const el of muted) if (!el.isConnected) { el.inert = false; muted.delete(el) }
}

function focusTop() {
  if (!interactive(top) || (document.activeElement !== document.body && interactive(document.activeElement as HTMLElement | null))) return
  const target = [...top.querySelectorAll<HTMLElement>('[data-autofocus]'), ...top.querySelectorAll<HTMLElement>('[data-modal-card]')].find(interactive)
  target?.focus({ preventScroll: true })
}

function tabModal(e: KeyboardEvent) {
  if (e.key !== 'Tab' || !interactive(top)) return
  // Inclui os portais de menus liberados pelo mesmo cálculo, sem sair para a página.
  const roots = [...document.body.children].filter((el): el is HTMLElement => el instanceof HTMLElement && interactive(el))
  const items = roots.flatMap((el) => [...el.querySelectorAll<HTMLElement>(FOCUSABLE)]).filter((el) => el.tabIndex >= 0 && interactive(el))
  if (!items.length) { e.preventDefault(); top.querySelector<HTMLElement>('[data-modal-card]')?.focus(); return }
  const active = document.activeElement
  const place = (el: HTMLElement) => el === active ? 'at' : active && active.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING ? 'after' : 'before'
  const wrap = trapWrap(items.map(place), e.shiftKey, roots.some((el) => el.contains(active)))
  if (wrap) { e.preventDefault(); items[wrap === 'first' ? 0 : items.length - 1]!.focus() }
}

function watchModals() {
  if (observer) return
  observer = new MutationObserver(() => { syncModals(); focusTop() })
  observer.observe(document.body, { childList: true })
  window.addEventListener('keydown', tabModal, true)
}

function stopWatching() {
  if (openModals.size || exiting.size) return
  observer?.disconnect()
  observer = null
  window.removeEventListener('keydown', tabModal, true)
}

export function useModalFocus(open: boolean, mounted: boolean, layer: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const node = layer.current
    if (!open || !node) return
    exiting.delete(node)
    openModals.set(node, document.activeElement instanceof HTMLElement ? document.activeElement : null)
    watchModals()
    syncModals()
    focusTop()
    return () => {
      const previous = openModals.get(node) ?? null
      const wasTop = top === node
      openModals.delete(node)
      // Se a folha inferior sumir primeiro, o último modal conserva a origem válida.
      for (const [other, origin] of openModals) if (origin && node.contains(origin)) openModals.set(other, previous)
      exiting.add(node)
      syncModals()
      if (wasTop && interactive(previous)) previous.focus({ preventScroll: true })
      focusTop()
    }
  }, [open, mounted, layer])
  useLayoutEffect(() => {
    const node = layer.current
    if (!mounted || !node) return
    return () => {
      exiting.delete(node)
      syncModals()
      stopWatching()
    }
  }, [mounted, layer])
}

/** Painel não modal continua montado durante sua animação de saída. */
export function useInertExit(open: boolean, mounted: boolean, layer: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const node = layer.current
    if (open || !mounted || !node) return
    exiting.add(node)
    syncModals()
    return () => { exiting.delete(node); syncModals(); stopWatching() }
  }, [open, mounted, layer])
}

interface EscapeLayer { layer: RefObject<HTMLElement | null>; run: () => void }
const escapes: EscapeLayer[] = []
/** Um listener escolhe antes do callback: desmontagem síncrona não consome outra camada. */
function escapeKey(e: KeyboardEvent) {
  if (e.key !== 'Escape' || e.defaultPrevented) return
  const entry = escapes.findLast(({ layer }) => interactive(layer.current))
  if (!entry) return
  e.preventDefault()
  e.stopImmediatePropagation()
  entry.run()
}

export function useLayerEscape(active: boolean, onEscape: () => void, layer: RefObject<HTMLElement | null>) {
  // A ref é obrigatória em todos os consumidores, inclusive páginas atrás de um modal.
  const handler = useRef(onEscape)
  handler.current = onEscape
  useLayoutEffect(() => {
    if (!active) return
    const entry = { layer, run: () => handler.current() }
    if (!escapes.length) window.addEventListener('keydown', escapeKey, true)
    escapes.push(entry)
    return () => {
      escapes.splice(escapes.indexOf(entry), 1)
      if (!escapes.length) window.removeEventListener('keydown', escapeKey, true)
    }
  }, [active, layer, handler])
}
