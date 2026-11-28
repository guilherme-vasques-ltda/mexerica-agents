// Entrada exclusiva da QA via Vite; não é importada pelo app nem usa um Hub real.
// Cenários do seletor Navegador (E1+E2) por query: ?nav=computer|integrated|off&computer=0|1&screen=0|1&theme=light|dark
import './bridge'
import { createRoot } from 'react-dom/client'
import type { HostTransport } from '../client/host-client'
import { FakeHost } from './fake-host'
import { AppModel } from '../store/app-model'
import { BotStore } from '../store/bot-store'
import { AppContext, StoreContext } from '../store/context'
import { idiomaPref } from '../lib/fork-i18n/estado'
import { applyTextSize } from '../lib/prefs'
import { appearancePrefs, startAppearance } from '../lib/appearance'
import { BotSettingsPage } from '../views/fork/bot-settings/BotSettingsPage'
import type { BrowserPreference } from '@shared/fork-browser'
import type { ScreenState } from '@shared/models'
import '../styles/tokens.css'
import '../styles/fonts.css'
import '../styles/theme.css'

const q = new URLSearchParams(location.search)
const nav = (q.get('nav') ?? 'computer') as BrowserPreference
const computer = q.get('computer') !== '0'
const comTela = q.get('screen') !== '0'
const tema = q.get('theme') === 'light' ? 'light' : 'dark'
const navegadorLocal = q.get('local') === '1'
const politicaGerenciada = q.get('policy') === 'managed'
const quadroIndisponivel = q.get('frame') === 'error'
let consentimento = false

const TELA: ScreenState = {
  enabled: true, connected: true, platform: 'macos', capture: true, input: true,
  displays: [{ id: 1, name: 'Built-in', width: 1512, height: 982, main: true }],
  userControl: false, agentUse: 'idle', agentBot: null, computerUse: true, viewers: 0,
}

const host = new FakeHost('2.20.3')
host.hostId = 'qa-navegador'
host.screenDemo = comTela ? TELA : null
host.fixtureBot('sprint', {
  browserPreference: nav,
  computer,
  screenAccess: [{ target: 'local', name: 'MacBook Pro', available: true, enabled: true, notice: '' }],
})

const transport: HostTransport = {
  async call(method, body) {
    const params = (body ?? {}) as Record<string, unknown>
    if (['botEditorSave', 'updateBot', 'createBot', 'setVoiceKey'].includes(method)) throw new Error('QA_WRITE_FORBIDDEN')
    if (navegadorLocal && method === 'donnaBrowserLocalState') {
      return { supported: true, canChange: true, enabled: consentimento, ready: !politicaGerenciada, reason: politicaGerenciada ? 'browser_managed_policy' : null }
    }
    if (navegadorLocal && method === 'donnaBrowserLocalConsent') {
      consentimento = params.enabled === true
      return { enabled: consentimento }
    }
    if (navegadorLocal && method === 'donnaBrowserLocalFrame') {
      if (!consentimento || params.hubId !== host.hostId || params.botId !== 'sprint') return { jpeg: null }
      if (quadroIndisponivel) throw new Error('QA_FRAME_UNAVAILABLE')
      const quadro = document.createElement('canvas')
      quadro.width = 800
      quadro.height = 450
      const ctx = quadro.getContext('2d')
      if (!ctx) return { jpeg: null }
      ctx.fillStyle = '#f4f5f7'
      ctx.fillRect(0, 0, quadro.width, quadro.height)
      ctx.fillStyle = '#293140'
      ctx.font = '32px sans-serif'
      ctx.fillText('Página de exemplo', 48, 86)
      ctx.font = '18px sans-serif'
      ctx.fillText('preview.example.invalid', 48, 128)
      return { jpeg: quadro.toDataURL('image/jpeg').split(',')[1] }
    }
    if (method === 'donnaBrowserStatus') {
      return {
        version: 2, browserPreference: nav, scope: { hubId: params.hubId, spaceId: params.spaceId, botId: params.botId },
        target: 'desktop', status: 'unavailable', reason: 'backend_absent',
        profiles: { status: 'unavailable' },
        capabilities: { persistentProfile: false, ephemeralProfile: false, observe: false, control: false, files: false, privateLogin: false },
        message: 'Navegador integrado indisponível.',
      }
    }
    return host.call(method, params)
  },
  stream(path, onData) { return host.subscribe(Number(new URL(path, 'http://qa.invalid').searchParams.get('since') ?? 0), onData) },
}

const store = new BotStore({ id: 'qa', name: 'Hub fictício QA', signKey: '', urls: [] }, true, () => transport)
store.start()
const app = new AppModel(false)
app.stores = new Map([['qa', store]])
app.computers = [store.computer]
app.host = { ...app.host, local: { computerId: 'qa', baseURL: 'http://qa.invalid', token: 'demo' } }
idiomaPref.set('pt-BR')
appearancePrefs.theme.set(tema)
applyTextSize(13)
startAppearance()

const root = createRoot(document.getElementById('root')!)
root.render(<AppContext value={app}><StoreContext value={store}>
  <BotSettingsPage botId="sprint" initialSection="behaviour" onClose={() => root.render(null)} onConversation={() => {}} />
</StoreContext></AppContext>)

Object.defineProperty(window, 'qaNav', { value: {
  cenario: { nav, computer, comTela, tema },
  encerrar() { root.unmount(); store.retire() },
} })
