// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { createContext, useCallback, useContext, useRef, useSyncExternalStore } from 'react'

/**
 * Telas que cobrem o chat (Ajustes, Rotinas, Extensões…) marcam a árvore coberta com este
 * contexto. Enquanto estiver true, os hooks deste módulo devolvem o último snapshot: a loja
 * continua viva, mas nada renderiza atrás da tela que cobre. Ao voltar, o snapshot fresco
 * dispara um único re-render de ressincronia. MX-4 (C11).
 */
export const SubscriptionPaused = createContext(false)

/** A model object views follow: `changed()` re-renders every view that uses it. */
export class Observable {
  #listeners = new Set<() => void>()
  #version = 0
  #scheduled = false

  /** Coalesced to one notification per microtask, like SwiftUI's batched updates. */
  protected changed() {
    if (this.#scheduled) return
    this.#scheduled = true
    queueMicrotask(() => {
      this.#scheduled = false
      this.#version++
      for (const listener of [...this.#listeners]) listener()
    })
  }

  subscribe = (listener: () => void) => {
    this.#listeners.add(listener)
    return () => void this.#listeners.delete(listener)
  }

  getVersion = () => this.#version
}

/** Re-renders the component when `model` changes; returns it for convenience. */
export function useModel<T extends Observable | null | undefined>(model: T): T {
  const paused = useContext(SubscriptionPaused)
  // Fakes de teste podem ser objetos simples, sem a API de observável: tratam-se como estáticos.
  const live = model && typeof model.getVersion === 'function' ? model : null
  const frozen = useRef<number | null>(null)
  if (paused && frozen.current === null) frozen.current = live ? live.getVersion() : 0
  if (!paused) frozen.current = null
  useSyncExternalStore(
    live ? live.subscribe : noopSubscribe,
    () => (frozen.current !== null ? frozen.current : live ? live.getVersion() : 0),
  )
  return model
}

const noopSubscribe = () => () => {}

/** Re-renders on a change to any of `models` (a list whose length may change). */
export function useModels(models: Observable[]) {
  const paused = useContext(SubscriptionPaused)
  const key = useRef<{ models: Observable[]; version: number }>({ models, version: 0 })
  key.current.models = models
  const frozen = useRef<number | null>(null)
  if (paused && frozen.current === null) frozen.current = key.current.version
  if (!paused) frozen.current = null
  useSyncExternalStore(
    useCallback((cb: () => void) => {
      const offs = key.current.models.map((m) => m.subscribe(() => {
        key.current.version++
        cb()
      }))
      return () => offs.forEach((off) => off())
    }, [models.length, ...models]),
    () => (frozen.current !== null ? frozen.current : key.current.version),
  )
}

/** O que um modelo precisa expor para `useSelector`/`useSelectMany` assinarem. */
export interface Subscribable {
  subscribe: (listener: () => void) => () => void
}

/**
 * Re-renders only when `select(model)` yields a value `equal` rejects (default `Object.is`).
 * For views that need a few fields of a broad model, not every `changed()`.
 */
export function useSelector<T extends Subscribable | null | undefined, S>(model: T, select: (model: T) => S, equal: (a: S, b: S) => boolean = Object.is): S {
  const paused = useContext(SubscriptionPaused)
  const cache = useRef<{ value: S } | null>(null)
  if (cache.current === null) cache.current = { value: select(model) }
  const getSnapshot = () => {
    if (paused) return cache.current!.value
    const next = select(model)
    const prev = cache.current!.value
    if (equal(prev, next)) return prev
    cache.current = { value: next }
    return next
  }
  return useSyncExternalStore(model ? model.subscribe : noopSubscribe, getSnapshot)
}

/** Tuplas comparadas elemento a elemento (Object.is): o comparador das assinaturas por fatia. */
export function listEqual(a: readonly unknown[], b: readonly unknown[]) {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false
  return true
}

/**
 * `useSelector` sobre uma lista de observáveis (lojas por computador, por exemplo): assina
 * todas, mas só renderiza quando `select()` — que lê a lista viva — muda de valor. A lista é
 * passada de novo a cada render; se a identidade mudar, a assinatura é refeita e o valor é
 * recalculado. MX-4 (C9).
 */
export function useSelectMany<S>(models: Subscribable[], select: () => S, equal: (a: S, b: S) => boolean = Object.is): S {
  const paused = useContext(SubscriptionPaused)
  const ref = useRef<{ models: Subscribable[]; has: boolean; value: S | undefined; dirty: boolean }>({ models, has: false, value: undefined, dirty: true })
  if (ref.current.models.length !== models.length || ref.current.models.some((m, i) => m !== models[i])) {
    ref.current = { models, has: ref.current.has, value: ref.current.value, dirty: true }
  }
  const getSnapshot = (): S => {
    const r = ref.current
    // Pausado, devolve sempre o congelado: um `changed()` atrás da tela que cobre não renderiza.
    if (r.has && (paused || !r.dirty)) return r.value as S
    const had = r.has
    const prev = r.value as S
    const next = select()
    r.has = true
    r.dirty = false
    if (had && equal(prev, next)) return prev
    r.value = next
    return next
  }
  useSyncExternalStore(
    useCallback((cb: () => void) => {
      const offs = ref.current.models.map((m) => m.subscribe(() => {
        ref.current.dirty = true
        cb()
      }))
      return () => offs.forEach((off) => off())
    }, [models.length, ...models]),
    getSnapshot,
  )
  return ref.current.value as S
}
