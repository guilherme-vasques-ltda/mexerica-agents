import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { Script, createContext } from 'node:vm'
import test from 'node:test'
import ts from 'typescript'

// PR96 Opus1 F3: exercise the production hook across hide, window focus and reconnect.
// React effects and the DOM events are local fakes; no Hub, receipt or real profile.
test('a conversa montada mas oculta não registra leitura no foco, evento ou reconexão', () => {
  const target = new EventTarget()
  let inView = false
  let effect: (() => void | (() => void)) | null = null
  const react = { useContext: () => inView, useId: () => 'view', useEffect: (fn: typeof effect) => { effect = fn }, useLayoutEffect: (fn: typeof effect) => { effect = fn } }
  const document = Object.assign(new EventTarget(), { visibilityState: 'visible', hasFocus: () => true })
  const active = new Set<string>()
  let receipts = 0
  const store = { connection: { kind: 'online' }, setReading(token: string, _bot: string, visible: boolean) { if (visible) { active.add(token); receipts++ } else active.delete(token) } }
  const source = new URL('./reading.ts', import.meta.url)
  const output = ts.transpileModule(readFileSync(source, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 } }).outputText
  const exports = {} as { useReading: (s: typeof store, bot: string) => void }
  new Script(output, { filename: source.pathname }).runInContext(createContext({ exports, window: target, document, require: (name: string) => name === 'react' ? react : { PersonalReadingVisible: {} } }))
  let cleanup: () => void = () => {}
  const render = (visible: boolean) => {
    cleanup()
    inView = visible
    exports.useReading(store, 'pessoal')
    cleanup = (effect as unknown as () => (() => void))()
  }
  render(false) // Space or Rotinas hides Pessoal, which stays mounted for its draft.
  target.dispatchEvent(new Event('focus'))
  document.dispatchEvent(new Event('visibilitychange'))
  store.connection.kind = 'offline'
  render(false)
  store.connection.kind = 'online'
  render(false)
  assert.equal(active.size, 0, 'new entries and reconnect find no active reader')
  assert.equal(receipts, 0, 'focus and reconnect never mark the hidden conversation')
  render(true)
  assert.equal(active.size, 1)
  assert.equal(receipts, 1, 'returning to Pessoal re-enables the visible reader')
  render(false)
  target.dispatchEvent(new Event('focus'))
  assert.equal(active.size, 0)
  assert.equal(receipts, 1)
  cleanup()
})
