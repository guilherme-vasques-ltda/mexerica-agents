// O app em si (antes em main.tsx). MX-93: main.tsx carrega este módulo depois de a abertura pintar.
import { createRoot } from 'react-dom/client'
import './styles/tokens.css'
import './styles/fonts.css'
import './styles/theme.css'
import './views/fork/toques-mexerica/toques.css'
import { applyTextSize, prefs } from './lib/prefs'
import { idiomaAtual, idiomaPref } from './lib/fork-i18n/estado'
import { registrarIdioma } from './store/fork-idioma-hub'
import { registrarIdiomaNoMain } from './store/fork-idioma-main'
import { startAppearance } from './lib/appearance'
import { iniciarPausaAmbiente } from './lib/animation-visibility'
import { errorReport } from '@shared/error-report'
import { nomeVisivel } from '@shared/fork-nome-visivel'
import { sairDaAbertura } from './lib/abertura'

// Error tracking: this window's uncaught errors go to the host (shared/error-report.ts).
const report = (error: unknown, mechanism: string) => window.codync.app.reportError(errorReport(error, 'desktop-renderer', mechanism))
// An error event without an error (the ResizeObserver loop notice) has nothing to report.
window.addEventListener('error', (e) => { if (e.error != null) report(e.error, 'onerror') })
window.addEventListener('unhandledrejection', (e) => report(e.reason, 'unhandledrejection'))

const page = new URLSearchParams(location.search).get('window') ?? 'chat'
document.title = nomeVisivel(window.codync.appName)
document.documentElement.dataset.platform = window.codync.platform
document.documentElement.dataset.window = page
applyTextSize(prefs.textSize.get())
registrarIdioma(idiomaAtual, idiomaPref.subscribe) // D395: o idioma do app vai ao Hub
// D454/D495: e ao processo principal (menu do Mac e barra de menus).
registrarIdiomaNoMain(idiomaAtual, idiomaPref.subscribe, (idioma) => window.codync.app.setLanguage?.(idioma))
// Theme, accent, wallpaper and glass (Aparência) before the first paint, in every window.
startAppearance()
// Animações CSS infinitas pausam com a janela oculta/sem foco e fora da vista (lib/animation-visibility.ts), em toda janela.
iniciarPausaAmbiente()
// MX-93: só a janela principal tem abertura; nas outras o nó do index.html sai já.
if (page !== 'chat') sairDaAbertura()
const root = createRoot(document.getElementById('root')!)

// Auditoria D249 (E2): cada janela importa só o próprio código (views/fork/sob-demanda/).
void (page === 'island' ? import('./views/fork/sob-demanda/janela-ilha') : import('./views/fork/sob-demanda/janela-app'))
  .then(({ abrir }) => abrir(root, page))
  .catch((error: unknown) => { sairDaAbertura(); report(error, 'window-import') })
