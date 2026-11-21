// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { vozBridge } from './fork-voz'
import { accountProfileBridge } from './fork-account-profile'
import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import { sshBridge } from './ssh'
import type { CallError, CodyncBridge, HostSnapshot, ReleaseNoticeState, TraySummary, WindowCommand } from '../shared/ipc'
import type { BrowserFrame } from '../shared/relay-protocol/browser-frames'

let streamId = 0
const streams = new Map<string, { onData: (line: string) => void; onEnd: (error: CallError | null) => void }>()

ipcRenderer.on('host:stream:data', (_e, id: string, lines: string[]) => {
  const s = streams.get(id)
  if (s) for (const line of lines) s.onData(line)
})
ipcRenderer.on('host:stream:end', (_e, id: string, error: CallError | null) => {
  const s = streams.get(id)
  streams.delete(id)
  s?.onEnd(error)
})

function listen<T>(channel: string, cb: (value: T) => void) {
  const handler = (_e: IpcRendererEvent, value: T) => cb(value)
  ipcRenderer.on(channel, handler)
  return () => void ipcRenderer.removeListener(channel, handler)
}

const bridge: CodyncBridge = {
  // MX-209: canais escritos aqui de propósito. Importar `shared/fork-cofre-seguro` faria o rollup criar um chunk
  // compartilhado com o preload da janela segura, e preload com sandbox não consegue `require` de outro arquivo
  // (o `window.codync` inteiro some). Os valores são os de `cofreCanais` (conferidos em fork-cofre-seguro.test.ts).
  forkCofre: {
    abrir: (pedido) => ipcRenderer.invoke('fork-cofre-seguro:abrir', pedido),
    abrirNovo: (pedido) => ipcRenderer.invoke('fork-cofre-seguro:abrir-novo', pedido),
    onEstado: (cb) => listen('fork-cofre-seguro:estado', cb),
  },
  voz: vozBridge,
  accountProfile: accountProfileBridge,
  platform: process.platform as CodyncBridge['platform'],
  appName: ipcRenderer.sendSync('app:name') as string,
  appScheme: ipcRenderer.sendSync('app:scheme') as string,
  hostPort: ipcRenderer.sendSync('app:hostPort') as number,
  appVersion: ipcRenderer.sendSync('app:version') as string,
  computerName: ipcRenderer.sendSync('app:computerName') as string,
  suggestedLanguage: ipcRenderer.sendSync('app:suggestedLanguage') as CodyncBridge['suggestedLanguage'],
  openedAtLogin: ipcRenderer.sendSync('app:openedAtLogin') as boolean,
  debugOpen: ipcRenderer.sendSync('app:debugOpen') as string | null,
  host: {
    snapshot: () => ipcRenderer.invoke('host:snapshot') as Promise<HostSnapshot>,
    onChange: (cb) => listen('host:change', cb),
    install: () => ipcRenderer.send('host:install'),
    restart: () => ipcRenderer.send('host:restart'),
    async call(baseURL, token, method, body, timeoutMs) {
      const result = (await ipcRenderer.invoke('host:call', baseURL, token, method, body, timeoutMs)) as
        | { ok: true; value: unknown }
        | { ok: false; error: CallError }
      if (!result.ok) throw result.error
      return result.value
    },
    stream(url, token, onData, onEnd) {
      const id = `s${++streamId}`
      streams.set(id, { onData, onEnd })
      ipcRenderer.send('host:stream:open', id, url, token)
      return () => {
        if (!streams.delete(id)) return
        ipcRenderer.send('host:stream:close', id)
      }
    },
  },
  forkCloud: {
    invoke: (intent) => ipcRenderer.invoke('forkCloud:invoke', intent),
    onChange: (cb) => listen('forkCloud:change', cb),
    call: (connectionId, method, body, timeoutMs) => ipcRenderer.invoke('forkCloud:call', connectionId, method, body, timeoutMs),
    subscribeEvents(connectionId, body, onEvent, onEnd) {
      const id = globalThis.crypto.randomUUID().replaceAll('-', '')
      let active = true
      const data = (_e: IpcRendererEvent, received: string, value: unknown) => { if (active && received === id) onEvent(value) }
      const end = (_e: IpcRendererEvent, received: string, error: CallError | null) => {
        if (!active || received !== id) return
        active = false; ipcRenderer.removeListener('forkCloud:events:data', data); ipcRenderer.removeListener('forkCloud:events:end', end); onEnd(error)
      }
      ipcRenderer.on('forkCloud:events:data', data); ipcRenderer.on('forkCloud:events:end', end)
      ipcRenderer.send('forkCloud:events:open', id, connectionId, body)
      return () => { if (!active) return; active = false; ipcRenderer.removeListener('forkCloud:events:data', data); ipcRenderer.removeListener('forkCloud:events:end', end); ipcRenderer.send('forkCloud:events:cancel', id) }
    },
    subscribeFrames(connectionId, onFrame, onEnd) {
      const id = globalThis.crypto.randomUUID().replaceAll('-', '')
      let active = true
      let open = (_value: boolean) => {}
      const opened = new Promise<boolean>(resolve => { open = resolve })
      const stop = () => { ipcRenderer.removeListener('forkCloud:frames:data', data); ipcRenderer.removeListener('forkCloud:frames:end', end); ipcRenderer.removeListener('forkCloud:frames:opened', ready) }
      const data = (_e: IpcRendererEvent, received: string, frame: BrowserFrame, waited: unknown) => { if (active && received === id) onFrame(frame, typeof waited === 'number' && Number.isFinite(waited) && waited >= 0 ? waited : Number.POSITIVE_INFINITY) }
      const end = (_e: IpcRendererEvent, received: string, error: CallError | null) => { if (!active || received !== id) return; active = false; stop(); open(false); onEnd(error) }
      const ready = (_e: IpcRendererEvent, received: string) => { if (active && received === id) open(true) }
      ipcRenderer.on('forkCloud:frames:data', data); ipcRenderer.on('forkCloud:frames:end', end); ipcRenderer.on('forkCloud:frames:opened', ready)
      ipcRenderer.send('forkCloud:frames:open', id, connectionId)
      return {
        opened,
        cancel: () => { if (!active) return; active = false; stop(); open(false); ipcRenderer.send('forkCloud:frames:cancel', id) },
        consumed: () => { if (active) ipcRenderer.send('forkCloud:frames:consumed', id) },
      }
    },
  },
  account: {
    state: () => ipcRenderer.invoke('account:state'),
    onChange: (cb) => listen('account:change', cb),
    signIn: (provider) => ipcRenderer.invoke('account:signIn', provider),
    signInWithPassword: (identifier, password) => ipcRenderer.invoke('account:signInWithPassword', identifier, password),
    reconcileSignInWithPassword: (identifier, password) => ipcRenderer.invoke('account:reconcileSignInWithPassword', identifier, password),
    attemptSecondFactor: (strategy, code) => ipcRenderer.invoke('account:attemptSecondFactor', strategy, code),
    signUpWithPassword: (email, password) => ipcRenderer.invoke('account:signUpWithPassword', email, password),
    signInWithEmailCode: (email) => ipcRenderer.invoke('account:signInWithEmailCode', email),
    verifyEmailCode: (code) => ipcRenderer.invoke('account:verifyEmailCode', code),
    resendEmailCode: () => ipcRenderer.invoke('account:resendEmailCode'),
    forgotPassword: (email) => ipcRenderer.invoke('account:forgotPassword', email),
    resetPassword: (password) => ipcRenderer.invoke('account:resetPassword', password),
    cancelPending: () => ipcRenderer.invoke('account:cancelPending'),
    openWaitlist: () => ipcRenderer.invoke('account:openWaitlist'),
    signOut: () => ipcRenderer.invoke('account:signOut'),
    reconciliationState: () => ipcRenderer.invoke('account:reconciliationState'),
    reconcile: (action) => ipcRenderer.invoke('account:reconcile', action),
    reconcileSignIn: (provider) => ipcRenderer.invoke('account:reconcileSignIn', provider),
  },
  speech: {
    available: ipcRenderer.sendSync('speech:available') as boolean,
    locales: () => ipcRenderer.sendSync('speech:locales') as string[],
    start: (locale) => ipcRenderer.send('speech:start', locale),
    stop: () => ipcRenderer.send('speech:stop'),
    onEvent: (cb) => listen('speech:event', cb),
  },
  ssh: sshBridge,
  island: {
    supported: ipcRenderer.sendSync('island:supported') as boolean,
    enabled: () => ipcRenderer.invoke('island:enabled'),
    setEnabled: (on) => ipcRenderer.invoke('island:setEnabled', on),
    setSummary: (summary) => ipcRenderer.send('island:summary', summary),
    state: () => ipcRenderer.invoke('island:state'),
    onSummary: (cb) => listen('island:summary', cb),
    onScreen: (cb) => listen('island:screen', cb),
    onHover: (cb) => listen('island:hover', cb),
    setShape: (rect) => ipcRenderer.send('island:shape', rect),
    setFocusable: (on) => ipcRenderer.send('island:focusable', on),
    setKeys: (on) => ipcRenderer.send('island:keys', on),
    onKey: (cb) => listen('island:key', cb),
    respond: (ref, entryId, optionId) => ipcRenderer.send('island:respond', ref, entryId, optionId),
    send: (ref, text, contentOrigin, nonce) => ipcRenderer.invoke('island:send', ref, text, contentOrigin, nonce),
    sendResult: (replyId, resultado) => ipcRenderer.send('island:sendResult', replyId, resultado),
    openBot: (ref) => ipcRenderer.send('island:openBot', ref),
    newBot: () => ipcRenderer.send('island:newBot'),
    openApp: () => ipcRenderer.send('island:openApp'),
  },
  cloud: {
    request: (method, path, body, signed) => ipcRenderer.invoke('cloud:request', method, path, body, signed),
  },
  updates: {
    state: () => ipcRenderer.invoke('updates:state'),
    onChange: (cb) => listen('updates:change', cb),
    check: () => ipcRenderer.send('updates:check'),
    install: () => ipcRenderer.send('updates:install'),
    setAutoCheck: (on) => ipcRenderer.send('updates:autoCheck', on),
    setAutoDownload: (on) => ipcRenderer.send('updates:autoDownload', on),
  },
  // Nível 1 de atualizações: canais escritos de propósito (mesma regra de forkCofre, MX-209).
  releaseNotice: {
    state: () => ipcRenderer.invoke('releaseNotice:state') as Promise<ReleaseNoticeState>,
    check: () => ipcRenderer.invoke('releaseNotice:check') as Promise<ReleaseNoticeState>,
    dismiss: () => ipcRenderer.invoke('releaseNotice:dismiss') as Promise<ReleaseNoticeState>,
    openDownload: () => ipcRenderer.invoke('releaseNotice:openDownload') as Promise<boolean>,
    onChange: (cb) => listen('releaseNotice:change', cb),
  },
  files: {
    begin: (file) => ipcRenderer.invoke('files:begin', file),
    write: (id, offset, data) => ipcRenderer.invoke('files:write', id, offset, data),
    finish: (id) => ipcRenderer.invoke('files:finish', id),
    cancel: (id) => ipcRenderer.invoke('files:cancel', id),
  },
  appIcon: {
    selected: () => ipcRenderer.invoke('appIcon:selected'),
    set: (id) => ipcRenderer.invoke('appIcon:set', id),
  },
  app: {
    reportError: (_report) => {},
    openExternal: (url) => ipcRenderer.send('app:openExternal', url),
    openMemoryFile: (path) => ipcRenderer.send('app:openMemoryFile', path),
    copy: (text) => ipcRenderer.send('app:copy', text),
    pickFiles: () => ipcRenderer.invoke('app:pickFiles'),
    readClipboardFiles: () => ipcRenderer.invoke('app:clipboardFiles'),
    setTraySummary: (summary: TraySummary) => ipcRenderer.send('app:traySummary', summary),
    setLanguage: (idioma) => ipcRenderer.send('app:setLanguage', idioma),
    onCommand: (cb: (c: WindowCommand) => void) => listen('app:command', cb),
    isFullScreen: () => ipcRenderer.invoke('app:isFullScreen'),
    setWindowControls: (cores) => ipcRenderer.send('app:windowControls', cores),
    resetAllData: () => ipcRenderer.invoke('app:resetAllData'),
    setLaunchAtLogin: (on) => ipcRenderer.invoke('app:setLaunchAtLogin', on),
    showInDock: () => ipcRenderer.invoke('app:showInDock'),
    setShowInDock: (on) => ipcRenderer.invoke('app:setShowInDock', on),
    launchAtLogin: () => ipcRenderer.invoke('app:launchAtLogin'),
    openSettings: (url) => ipcRenderer.send('app:openSettings', url),
    authenticate: (url, scheme) => ipcRenderer.invoke('app:authenticate', url, scheme),
    loginRetorno: {
      abrir: () => ipcRenderer.invoke('login-retorno:abrir'),
      esperar: (tentativa, url) => ipcRenderer.invoke('login-retorno:esperar', tentativa, url),
      responder: (tentativa, resultado) => ipcRenderer.send('login-retorno:responder', tentativa, resultado),
      fechar: (tentativa) => ipcRenderer.send('login-retorno:fechar', tentativa),
    },
    screenAgentState: () => ipcRenderer.invoke('screen:state'),
    setScreenAgent: (on) => ipcRenderer.invoke('screen:setAgent', on),
    syncScreenAgent: (enabled) => ipcRenderer.invoke('screen:syncAgent', enabled),
  },
}

contextBridge.exposeInMainWorld('codync', bridge)
