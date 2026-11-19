import { useEffect, type RefObject } from 'react'

/** Um serviço por documento, criado pelo primeiro spinner com Toques ligados ou pela pausa ambiente do app. */
const registries = new WeakMap<Document, ReturnType<typeof createRegistry>>()

/** Janela "ativa" = documento visível e com foco. Sem `hasFocus` (ambiente simples), só a visibilidade conta. */
const janelaAtiva = (doc: Document) => doc.visibilityState === 'visible' && (doc.hasFocus?.() ?? true)

/** Elemento de origem da animação (para ::before/::after, é o elemento dono do pseudo). */
const alvo = (animation: Animation) => (animation.effect as KeyframeEffect | null)?.target ?? null
const infinita = (animation: Animation) => animation.effect?.getComputedTiming().iterations === Infinity

function createRegistry(doc: Document) {
  const view = doc.defaultView!
  const entries = new Map<Element, boolean>()
  // Elementos descobertos por `animationstart` (pausa ambiente): só as animações infinitas deles são geridas,
  // guardadas aqui para as mutações não precisarem perguntar ao motor de estilo (getAnimations força estilo).
  const ambient = new Map<Element, Animation[]>()
  const paused = new WeakSet<Animation>()
  const animationsOf = (element: Element) => {
    const found = ambient.get(element)
    return found ? found.filter(animation => animation.playState !== 'idle') : element.getAnimations()
  }
  const update = (element: Element) => {
    const visible = janelaAtiva(doc) && entries.get(element)
      && element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })
    for (const animation of animationsOf(element)) {
      if (visible && paused.has(animation) && animation.playState === 'paused') {
        paused.delete(animation)
        animation.play()
      } else if (!visible && animation.playState === 'running') {
        paused.add(animation)
        animation.pause()
      }
    }
  }
  const release = (element: Element) => {
    for (const animation of animationsOf(element)) if (paused.has(animation)) {
      paused.delete(animation)
      if (animation.playState === 'paused') animation.play()
    }
  }
  const drop = (element: Element) => {
    if (!entries.delete(element)) return false
    ambient.delete(element)
    intersection.unobserve(element)
    release(element)
    return true
  }
  // Elemento removido do DOM sem nunca ter cruzado a viewport não gera aviso do IntersectionObserver: varre na troca de estado.
  const sweep = () => { for (const element of [...entries.keys()]) if (element.isConnected === false) drop(element) }
  const updateAll = () => { sweep(); for (const element of entries.keys()) update(element) }
  const updateWithin = (nodes: readonly Node[]) => {
    for (const element of entries.keys()) if (nodes.some(node => node.contains(element))) update(element)
  }
  const intersection: IntersectionObserver = new view.IntersectionObserver(changes => {
    for (const change of changes) {
      if (!entries.has(change.target)) continue
      if (change.target.isConnected === false) { drop(change.target); continue }
      entries.set(change.target, change.isIntersecting && change.intersectionRect.width > 0 && change.intersectionRect.height > 0)
      update(change.target)
    }
  })
  // Um único observador filtra somente mutações que afetam ancestrais dos elementos registrados.
  const mutations = new view.MutationObserver(changes => updateWithin(changes.map(change => change.target)))
  mutations.observe(doc.documentElement, { subtree: true, attributes: true, attributeFilter: ['class', 'style', 'hidden', 'inert'] })
  const afterTransition = (event: Event) => { if (event.target) updateWithin([event.target as Node]) }
  doc.addEventListener('visibilitychange', updateAll)
  // Janela sem foco (visível, mas a pessoa está em outro app): nada de loop contínuo redesenhando o vidro.
  view.addEventListener('focus', updateAll)
  view.addEventListener('blur', updateAll)
  doc.addEventListener('transitionend', afterTransition, true)
  doc.addEventListener('animationend', afterTransition, true)
  // O CSS elimina o giro em movimento reduzido. Ao sair desse modo, o novo arco
  // também deve começar pausado caso esteja oculto; uma única escuta cobre todos.
  const reduced = view.matchMedia('(prefers-reduced-motion: reduce)')
  reduced.addEventListener('change', updateAll)

  // Pausa ambiente: toda animação CSS infinita que começar no documento entra no gate.
  let ambientOn = false
  const pending = new Set<Element>()
  let sweepAt = 64
  const flush = () => {
    for (const element of pending) {
      const infinitas = element.getAnimations({ subtree: true }).filter(animation => infinita(animation) && alvo(animation) === element)
      if (!infinitas.length) continue
      if (!entries.has(element)) {
        // Começa "visível": o primeiro aviso do IntersectionObserver (próximo quadro) corrige, sem piscar a animação.
        entries.set(element, true)
        intersection.observe(element)
      }
      ambient.set(element, infinitas)
      update(element)
    }
    pending.clear()
    if (entries.size >= sweepAt) { sweep(); sweepAt = Math.max(64, entries.size * 2) }
  }
  const onAnimationStart = (event: Event) => {
    const target = event.target as Element | null
    if (!target || typeof (target as Element).getAnimations !== 'function') return
    if (!pending.size) queueMicrotask(flush)
    pending.add(target)
  }

  const disposeIfIdle = () => {
    if (entries.size || ambientOn) return
    intersection.disconnect()
    mutations.disconnect()
    doc.removeEventListener('visibilitychange', updateAll)
    view.removeEventListener('focus', updateAll)
    view.removeEventListener('blur', updateAll)
    doc.removeEventListener('transitionend', afterTransition, true)
    doc.removeEventListener('animationend', afterTransition, true)
    reduced.removeEventListener('change', updateAll)
    registries.delete(doc)
  }
  return {
    observe(element: Element) {
      if (!entries.has(element)) { entries.set(element, false); intersection.observe(element) }
      update(element)
      return () => { if (drop(element)) disposeIfIdle() }
    },
    observeAmbient() {
      if (ambientOn) return () => {}
      ambientOn = true
      doc.addEventListener('animationstart', onAnimationStart, true)
      // Animações que já corriam antes de a pausa ligar.
      for (const animation of doc.getAnimations()) { const dono = alvo(animation); if (dono) pending.add(dono) }
      if (pending.size) queueMicrotask(flush)
      return () => {
        if (!ambientOn) return
        ambientOn = false
        doc.removeEventListener('animationstart', onAnimationStart, true)
        pending.clear()
        for (const element of [...ambient.keys()]) drop(element)
        disposeIfIdle()
      }
    },
  }
}

function registryOf(doc: Document) {
  let registry = registries.get(doc)
  if (!registry) { registry = createRegistry(doc); registries.set(doc, registry) }
  return registry
}

/** Pausa Web Animations sem escrever atributos/estilos no controle. */
export function observeAnimationVisibility(element: Element) {
  return registryOf(element.ownerDocument).observe(element)
}

/**
 * Pausa ambiente do app: toda animação CSS infinita (avatares, pontos de estado, halo, giros...) fica pausada com a
 * janela oculta ou sem foco e quando o elemento sai da vista; retoma ao voltar. Não escreve em atributo nem estilo e
 * não mexe em animação finita. Movimento reduzido segue pelo CSS (sem animação, nada a pausar). Devolve o desligamento.
 */
export function iniciarPausaAmbiente(doc: Document = document) {
  return registryOf(doc).observeAmbient()
}

export function useVisibleAnimation(ref: RefObject<Element | null>, enabled: boolean) {
  useEffect(() => enabled && ref.current ? observeAnimationVisibility(ref.current) : undefined, [ref, enabled])
}
