import assert from 'node:assert/strict'
import { test } from 'node:test'
import { iniciarPausaAmbiente, observeAnimationVisibility } from './animation-visibility.ts'

function fixture() {
  let intersect: IntersectionObserverCallback = () => {}
  let mutate: MutationCallback = () => {}
  const counts = { intersection: 0, mutation: 0, disconnected: 0, unobserved: 0, rootObservations: 0 }
  const events = new Map<string, EventListener>()
  const media = new Map<string, EventListener>()
  const windowEvents = new Map<string, EventListener>()
  const root = { contains: () => true }
  const running: unknown[] = []
  const doc = { documentElement: root, visibilityState: 'visible', focused: true,
    hasFocus() { return this.focused },
    getAnimations: () => running as Animation[],
    addEventListener: (name: string, listener: EventListener) => events.set(name, listener),
    removeEventListener: (name: string) => events.delete(name),
    defaultView: {
      addEventListener: (name: string, listener: EventListener) => windowEvents.set(name, listener),
      removeEventListener: (name: string) => windowEvents.delete(name),
      IntersectionObserver: class {
        constructor(fn: IntersectionObserverCallback) { intersect = fn; counts.intersection++ }
        observe() {} unobserve() { counts.unobserved++ } disconnect() { counts.disconnected++ }
      },
      MutationObserver: class {
        constructor(fn: MutationCallback) { mutate = fn; counts.mutation++ }
        observe(node: unknown) { assert.equal(node, root); counts.rootObservations++ }
        disconnect() { counts.disconnected++ }
      },
      matchMedia: () => ({ addEventListener: (name: string, listener: EventListener) => media.set(name, listener), removeEventListener: (name: string) => media.delete(name) }),
    },
  }
  const createElement = () => {
    let cssVisible = true
    let reads = 0
    const animation = { playState: 'running', play() { this.playState = 'running' }, pause() { this.playState = 'paused' } }
    const element = { ownerDocument: doc, checkVisibility: () => { reads++; return cssVisible }, getAnimations: () => [animation] } as unknown as Element
    return { element, animation, hide: (hidden: boolean) => { cssVisible = !hidden }, reads: () => reads }
  }
  // Elemento com animações infinitas (inclui a de um ::after) e uma finita, como o ldot e o avatar reais.
  const createAmbient = () => {
    const make = (iterations: number, target: () => unknown) => ({ playState: 'running', effect: { getComputedTiming: () => ({ iterations }), get target() { return target() } },
      play() { this.playState = 'running' }, pause() { this.playState = 'paused' } })
    let cssVisible = true
    const element: Record<string, unknown> = { ownerDocument: doc, isConnected: true, checkVisibility: () => cssVisible }
    const own = make(Infinity, () => element), pseudo = make(Infinity, () => element), once = make(1, () => element)
    const other = make(Infinity, () => ({}))
    element.getAnimations = (options?: { subtree?: boolean }) => (options?.subtree ? [own, pseudo, once, other] : [own, once]) as unknown as Animation[]
    running.push(own, pseudo, once)
    return { element: element as unknown as Element, own, pseudo, once, other, hide: (hidden: boolean) => { cssVisible = !hidden } }
  }
  return { counts, events, windowEvents, media, doc, createElement, createAmbient,
    start: (element: Element) => events.get('animationstart')!({ target: element } as unknown as Event),
    enter: (element: Element, visible: boolean) => intersect([{ target: element, isIntersecting: visible, intersectionRect: { width: visible ? 14 : 0, height: visible ? 14 : 0 } } as IntersectionObserverEntry], {} as IntersectionObserver),
    mutate: (target = root) => mutate([{ target } as unknown as MutationRecord], {} as MutationObserver),
  }
}

test('D530 R2: serviço pausa fora da tela, em janela oculta e ancestral oculto; cleanup é completo', () => {
  const f = fixture(), one = f.createElement()
  const stop = observeAnimationVisibility(one.element)
  assert.equal(one.animation.playState, 'paused')
  f.enter(one.element, true)
  assert.equal(one.animation.playState, 'running')
  f.doc.visibilityState = 'hidden'; f.events.get('visibilitychange')!(new Event('visibilitychange'))
  assert.equal(one.animation.playState, 'paused')
  f.doc.visibilityState = 'visible'; f.events.get('visibilitychange')!(new Event('visibilitychange'))
  assert.equal(one.animation.playState, 'running')
  one.hide(true); f.mutate()
  assert.equal(one.animation.playState, 'paused')
  one.hide(false); f.mutate()
  assert.equal(one.animation.playState, 'running')
  f.enter(one.element, false)
  assert.equal(one.animation.playState, 'paused')
  stop()
  assert.equal(f.counts.disconnected, 2)
  assert.equal(f.events.size, 0)
  assert.equal(f.media.size, 0)
})

test('D530 R2: dois spinners compartilham um IO/MO e listeners; desligar o último libera o serviço', () => {
  const f = fixture(), one = f.createElement(), two = f.createElement()
  const stopOne = observeAnimationVisibility(one.element), stopTwo = observeAnimationVisibility(two.element)
  assert.deepEqual(f.counts, { intersection: 1, mutation: 1, disconnected: 0, unobserved: 0, rootObservations: 1 })
  assert.equal(f.events.size, 3)
  assert.equal(f.media.size, 1)
  f.enter(one.element, true); f.enter(two.element, true)
  f.enter(one.element, false)
  assert.equal(one.animation.playState, 'paused')
  assert.equal(two.animation.playState, 'running')
  const before = two.reads()
  f.mutate({ contains: () => false })
  assert.equal(two.reads(), before, 'mutações alheias não forçam estilo nos arcos')
  stopOne(); stopOne()
  assert.equal(f.counts.disconnected, 0)
  assert.equal(f.counts.unobserved, 1)
  assert.equal(f.events.size, 3)
  stopTwo()
  assert.equal(f.counts.disconnected, 2)
  assert.equal(f.events.size, 0)
  assert.equal(f.media.size, 0)
  const stopNew = observeAnimationVisibility(f.createElement().element)
  assert.equal(f.counts.intersection, 2, 'o próximo spinner recria o serviço depois da limpeza')
  stopNew()
})

const microtask = () => new Promise<void>(resolve => queueMicrotask(resolve))

test('Pausa ambiente: sem foco (blur) pausa mesmo visível e dentro da vista; focus retoma', () => {
  const f = fixture(), one = f.createElement()
  const stop = observeAnimationVisibility(one.element)
  f.enter(one.element, true)
  assert.equal(one.animation.playState, 'running')
  f.doc.focused = false; f.windowEvents.get('blur')!(new Event('blur'))
  assert.equal(f.doc.visibilityState, 'visible')
  assert.equal(one.animation.playState, 'paused')
  f.doc.focused = true; f.windowEvents.get('focus')!(new Event('focus'))
  assert.equal(one.animation.playState, 'running')
  // Voltar ao foco com o elemento fora da vista não o religa.
  f.enter(one.element, false)
  f.doc.focused = false; f.windowEvents.get('blur')!(new Event('blur'))
  f.doc.focused = true; f.windowEvents.get('focus')!(new Event('focus'))
  assert.equal(one.animation.playState, 'paused')
  stop()
  assert.equal(f.windowEvents.size, 0, 'cleanup remove focus e blur')
})

test('Pausa ambiente: pausa ambiente adota só animações infinitas (inclusive de pseudo-elemento) e segue foco, vista e retomada', async () => {
  const f = fixture(), one = f.createAmbient()
  const stop = iniciarPausaAmbiente(f.doc as unknown as Document)
  await microtask()
  assert.equal(f.counts.intersection, 1)
  assert.equal(one.own.playState, 'running', 'começa visível: sem pausar e religar')
  f.start(one.element); await microtask()
  f.enter(one.element, true)
  f.doc.focused = false; f.windowEvents.get('blur')!(new Event('blur'))
  assert.equal(one.own.playState, 'paused')
  assert.equal(one.pseudo.playState, 'paused', '::after do ldot também')
  assert.equal(one.once.playState, 'running', 'animação finita não é tocada')
  assert.equal(one.other.playState, 'running', 'animação de descendente não é tocada')
  f.doc.focused = true; f.windowEvents.get('focus')!(new Event('focus'))
  assert.equal(one.own.playState, 'running')
  assert.equal(one.pseudo.playState, 'running')
  f.doc.visibilityState = 'hidden'; f.events.get('visibilitychange')!(new Event('visibilitychange'))
  assert.equal(one.own.playState, 'paused')
  f.doc.visibilityState = 'visible'; f.events.get('visibilitychange')!(new Event('visibilitychange'))
  assert.equal(one.own.playState, 'running')
  f.enter(one.element, false)
  assert.equal(one.own.playState, 'paused', 'fora da vista')
  f.enter(one.element, true)
  assert.equal(one.own.playState, 'running')
  stop()
  assert.equal(f.counts.disconnected, 2)
  assert.equal(f.events.size, 0)
  assert.equal(f.windowEvents.size, 0)
})

test('Pausa ambiente: animação que começa com a janela sem foco já nasce pausada; ao desligar, devolve tudo rodando', async () => {
  const f = fixture(), one = f.createAmbient()
  const stop = iniciarPausaAmbiente(f.doc as unknown as Document)
  f.doc.focused = false
  f.start(one.element); await microtask()
  assert.equal(one.own.playState, 'paused')
  stop()
  assert.equal(one.own.playState, 'running')
})

test('Pausa ambiente: mutação em ancestral reavalia o elemento ambiente (visibility:hidden não muda a interseção)', async () => {
  const f = fixture(), one = f.createAmbient()
  const stop = iniciarPausaAmbiente(f.doc as unknown as Document)
  f.start(one.element); await microtask()
  f.enter(one.element, true)
  one.hide(true); f.mutate()
  assert.equal(one.own.playState, 'paused')
  one.hide(false); f.mutate()
  assert.equal(one.own.playState, 'running')
  stop()
})

test('Pausa ambiente: movimento reduzido — mudar a preferência reavalia, e animação cancelada pelo CSS nunca é religada', async () => {
  const f = fixture(), one = f.createAmbient()
  const stop = iniciarPausaAmbiente(f.doc as unknown as Document)
  f.start(one.element); await microtask()
  f.enter(one.element, true)
  f.doc.focused = false; f.windowEvents.get('blur')!(new Event('blur'))
  assert.equal(one.own.playState, 'paused')
  // CSS de reduce-motion cancela a animação (playState idle); voltar ao foco não pode ressuscitá-la.
  one.own.playState = 'idle'
  f.doc.focused = true; f.windowEvents.get('focus')!(new Event('focus'))
  assert.equal(one.own.playState, 'idle')
  assert.equal(one.pseudo.playState, 'running')
  // A mudança da preferência passa pelo mesmo reavaliador.
  one.hide(true); f.media.get('change')!(new Event('change'))
  assert.equal(one.pseudo.playState, 'paused')
  one.hide(false); f.media.get('change')!(new Event('change'))
  assert.equal(one.pseudo.playState, 'running')
  stop()
})

test('Pausa ambiente: elemento removido do DOM é solto e devolve a animação; serviço compartilhado com spinner e ligado só uma vez', async () => {
  const f = fixture(), one = f.createAmbient(), spinner = f.createElement()
  const stopSpinner = observeAnimationVisibility(spinner.element)
  const stop = iniciarPausaAmbiente(f.doc as unknown as Document)
  const again = iniciarPausaAmbiente(f.doc as unknown as Document); again()
  assert.equal(f.counts.intersection, 1)
  f.start(one.element); await microtask()
  f.doc.focused = false; f.windowEvents.get('blur')!(new Event('blur'))
  assert.equal(one.own.playState, 'paused')
  assert.equal(spinner.animation.playState, 'paused')
  ;(one.element as unknown as { isConnected: boolean }).isConnected = false
  f.doc.focused = true; f.windowEvents.get('focus')!(new Event('focus'))
  assert.equal(one.own.playState, 'running', 'solto ao varrer')
  stop()
  assert.equal(f.counts.disconnected, 0, 'o spinner ainda usa o serviço')
  stopSpinner()
  assert.equal(f.counts.disconnected, 2)
})
