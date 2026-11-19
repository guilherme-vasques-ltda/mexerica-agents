// Entrada exclusiva da QA via Vite; não é importada pelo app nem usa um Hub real.
import './bridge'
import { createRoot } from 'react-dom/client'
import type { HostTransport } from '../client/host-client'
import { FakeHost } from './fake-host'
import { AppModel } from '../store/app-model'
import { BotStore } from '../store/bot-store'
import { AppContext, StoreContext } from '../store/context'
import { estadoVozHub } from '../views/fork/voz/estado-hub'
import { estadoVozAparelho } from '../views/fork/voz/estado-aparelho'
import { idiomaPref } from '../lib/fork-i18n/estado'
import { applyTextSize } from '../lib/prefs'
import { startAppearance } from '../lib/appearance'
import { BotSettingsPage } from '../views/fork/bot-settings/BotSettingsPage'
import '../styles/tokens.css'
import '../styles/fonts.css'
import '../styles/theme.css'

const keys: { key: string; prevented: boolean }[] = []
document.addEventListener('keydown', e => {
  if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) keys.push({ key: e.key, prevented: e.defaultPrevented })
})
const events: { hub: string; method: string; provider?: unknown }[] = []
const active: Record<string, number> = { aparelho: 0 }
const configured = new URLSearchParams(location.search).get('configured') === 'true'

// Espiona o ciclo real dos observadores sem substituir a implementação.
function observar(id: string, estado: { observar(): () => void }) {
  active[id] = 0
  const original = estado.observar.bind(estado)
  estado.observar = () => {
    const stop = original()
    active[id]++
    let on = true
    return () => { if (on) { on = false; active[id]--; stop() } }
  }
}

function hub(id: string, loopback: boolean) {
  const host = new FakeHost('2.20.3')
  host.hostId = `qa-${id}`
  const transport: HostTransport = {
    async call(method, body) {
      const params = (body ?? {}) as Record<string, unknown>
      if (['botEditorSave', 'updateBot', 'createBot', 'setVoiceKey'].includes(method)) throw new Error('QA_WRITE_FORBIDDEN')
      if (method.startsWith('voice') || method === 'setVoiceKey') {
        events.push({ hub: id, method, ...(params.provider ? { provider: params.provider } : {}) })
        if (method === 'voiceStatus') return { providers: [{ provider: 'openai', configured }] }
        if (method === 'voiceModels') return { models: ['qa-realtime'], transcribe: ['qa-transcribe'], speech: ['qa-speech'] }
        throw new Error('QA_VOICE_EFFECT_FORBIDDEN')
      }
      const result = await host.call(method, params)
      return method === 'hello' ? { ...(result as object), name: `Hub fictício ${id}` } : result
    },
    stream(path, onData) { return host.subscribe(Number(new URL(path, 'http://qa.invalid').searchParams.get('since') ?? 0), onData) },
  }
  const store = new BotStore({ id, name: `Hub fictício ${id}`, signKey: '', urls: [] }, loopback, () => transport)
  observar(id, estadoVozHub(store))
  store.start()
  return store
}

observar('aparelho', estadoVozAparelho())
const stores = { a: hub('a', true), b: hub('b', true), c: hub('c', false) }
const app = new AppModel(false)
app.stores = new Map(Object.entries(stores))
app.computers = Object.values(stores).map(s => s.computer)
app.host = { ...app.host, local: { computerId: 'a', baseURL: 'http://qa.invalid', token: 'demo' } }
idiomaPref.set('pt-BR')
applyTextSize(13)
startAppearance()
const root = createRoot(document.getElementById('root')!)
let chosen: 'a' | 'b' = 'a'
function mount() {
  root.render(<AppContext value={app}><StoreContext value={stores[chosen]}>
    <BotSettingsPage key={chosen} botId="sprint" onClose={() => root.render(null)} onConversation={() => {}} />
  </StoreContext></AppContext>)
}
Object.defineProperty(window, 'qaVoz', { value: {
  configured, events, active, keys,
  store(id: 'a' | 'b') { chosen = id; mount() },
  idioma: (value: 'pt-BR' | 'ar-AE') => idiomaPref.set(value),
  desmontar() { root.render(null) },
  encerrar() { root.unmount(); Object.values(stores).forEach(store => store.retire()) },
} })
mount()
