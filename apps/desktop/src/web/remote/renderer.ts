import type { CodyncBridge } from '../../shared/ipc.ts'
import { instalarIdiomaWeb } from '../idioma.ts'
import { createWebBridge } from '../platform.ts'
import '../shell/shell.css'

const request = (window.parent as Window & {
  remoteRenderer?: (caller: Window) => { storage: Storage; host: CodyncBridge['host'] } | undefined
}).remoteRenderer
const context = window.parent !== window && request?.(window)
if (context) {
  Object.defineProperties(window, {
    localStorage: { value: context.storage },
    sessionStorage: { value: undefined },
    indexedDB: { value: undefined },
  })
  window.codync = createWebBridge({ host: context.host })
  instalarIdiomaWeb(window.codync)
  document.documentElement.dataset.webQueda = 'inerte'
  await import('../../renderer/main.tsx')
}
