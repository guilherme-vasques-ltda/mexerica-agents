// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { Terminal } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'
import { useEffect, useRef, useState } from 'react'
import { fromBase64, HostError, toBase64 } from '../../client/host-client'
import { Button } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { SheetPage } from '../../components/Overlay'
import { HStack, Spacer } from '../../components/Stack'
import { Text } from '../../components/Text'
import { ThinkingOrb } from '../../components/ThinkingOrb'
import { useStore } from '../../store/context'
import type { Backend } from '@shared/models'
import { errorText, market, type AuthMethod, type SetupStep } from './market-models'
import { ScreenHeader } from './pieces'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

// Só os motores com terminal de login remoto; o Hub aplica a mesma lista.
const loginHosts = new Set(['accounts.google.com', 'cursor.com', 'auth.x.ai', 'accounts.x.ai'])
const openLink = (uri: string, login: boolean) => {
  try {
    const url = new URL(uri)
    if (login && (url.protocol !== 'https:' || !loginHosts.has(url.hostname) || url.username || url.password || url.hash)) return
    if (/^https?:/i.test(uri)) window.codync.app.openExternal(uri)
  } catch {}
}

/**
 * Installing or signing in to an agent, or adding Node.js, live: the command runs in a
 * terminal on the computer and this screen is that terminal (links open here).
 */
export function SetupTerminalView({ backend, step, method, back }: {
  /** Null only for the `node` step, which belongs to the computer. */
  backend: Backend | null
  step: SetupStep
  /** A terminal sign-in method the agent offered; null runs Codync's own command. */
  method?: AuthMethod | null
  /** Back to the agent's sheet (the terminal is a step inside it). */
  back: () => void
}) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const surface = useRef<HTMLDivElement>(null)
  const [exitCode, setExitCode] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const el = surface.current
    if (!el) return
    // Root colors are already resolved; the Sheet has a different local ground.
    const css = getComputedStyle(document.documentElement)
    const scale = parseFloat(css.getPropertyValue('--scale')) || 1
    let restrictedLinks = false
    let waitingMessageShown = false
    const term = new Terminal({
      fontFamily: css.getPropertyValue('--font-mono').trim() || 'monospace',
      fontSize: 12 * scale,
      cursorBlink: true,
      allowTransparency: true,
      theme: {
        background: css.getPropertyValue('--canvas').trim(),
        foreground: css.getPropertyValue('--ink').trim(),
        cursor: css.getPropertyValue('--ink').trim(),
        selectionBackground: `rgba(${css.getPropertyValue('--ink-rgb').trim()}, 0.2)`,
      },
      linkHandler: { activate: (_e, uri) => openLink(uri, restrictedLinks) },
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new WebLinksAddon((_e, uri) => openLink(uri, restrictedLinks)))
    term.open(el)
    fit.fit()
    term.focus()

    let cancelled = false
    let exited: number | null = null
    let termId: string | null = null
    let activeClient: NonNullable<typeof store.client> | null = null
    let size = { cols: term.cols, rows: term.rows }
    let sent = { cols: 0, rows: 0 }
    let stopStream: (() => void) | null = null
    // One sender, so keystrokes can't overtake each other.
    let typing = Promise.resolve()
    const encoder = new TextEncoder()

    const syncSize = () => {
      const client = store.client
      if (!client || !termId || (size.cols === sent.cols && size.rows === sent.rows)) return
      sent = size
      void market.termResize(client, termId, size.cols, size.rows).catch(() => {})
    }
    const type = (bytes: Uint8Array) => {
      const client = store.client
      if (exited !== null || !client || !termId) return
      const id = termId
      typing = typing.then(() => market.termInput(client, id, toBase64(bytes)).then(() => {}, () => {}))
    }
    const subs = [
      term.onData((d) => type(encoder.encode(d))),
      term.onBinary((d) => type(Uint8Array.from(d, (c) => c.charCodeAt(0) & 0xff))),
      term.onResize(({ cols, rows }) => {
        size = { cols, rows }
        syncSize()
      }),
    ]
    const observer = new ResizeObserver(() => fit.fit())
    observer.observe(el)

    let attempt = 0
    /** Attaches to the output until it ends: `retry` after a dropped connection, `done` otherwise. */
    const attach = (client: NonNullable<typeof store.client>, id: string) =>
      new Promise<'retry' | 'done'>((resolve) => {
        term.reset()
        stopStream = client.transport.stream(
          `/term/${encodeURIComponent(id)}`,
          (line) => {
            attempt = 0
            let raw: { type: string; data?: string; code?: number | string; cliOnly?: boolean }
            try {
              raw = JSON.parse(line)
            } catch {
              return
            }
            if (raw.type === 'output' && raw.data) term.write(fromBase64(raw.data))
            else if (raw.type === 'notice' && raw.code === 'login_code_rejected') {
              term.write(`\r\n${t('extensoes.terminal.codigoRecusado')}\r\n`)
            } else if (raw.type === 'notice' && raw.code === 'login_waiting' && !waitingMessageShown) {
              waitingMessageShown = true
              term.write(`\r\n${t('extensoes.terminal.loginAguardando')}\r\n`)
            } else if (raw.type === 'verified') {
              exited = 0
              setExitCode(0)
              resolve('done')
              back()
            } else if (raw.type === 'exit') {
              exited = typeof raw.code === 'number' ? raw.code : -1
              setExitCode(exited)
              resolve('done')
            }
          },
          (e) => {
            stopStream = null
            if (step === 'login') {
              if (exited === null) {
                setError(t('extensoes.terminal.loginInterrompido'))
                void market.termClose(client, id).catch(() => {})
              }
              resolve('done')
            } else if (e?.status === 404) {
              setError(e.message)
              resolve('done')
            } else if (e) {
              attempt += 1
              if (attempt > 5) {
                setError(e.message)
                resolve('done')
              } else resolve('retry')
            } else resolve('retry')
          },
        )
      })

    void (async () => {
      try {
        const client = await store.ready()
        activeClient = client
        const setup = await market.agentSetup(client, backend?.id ?? '', step, method?.id ?? null, size.cols, size.rows)
        const id = setup.term
        restrictedLinks = setup.restrictedLinks === true
        if (cancelled) {
          void market.termClose(client, id).catch(() => {})
          return
        }
        termId = id
        sent = size
        syncSize()
        // Reattach after a dropped connection; the host replays the whole scrollback.
        while (exited === null && !cancelled) {
          if ((await attach(client, id)) === 'done') break
          if (exited === null && !cancelled) await new Promise((r) => setTimeout(r, 1000))
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof HostError ? e.message : errorText(e))
        return
      }
      if (!cancelled) await store.refreshBackends()
    })()

    // Leaving the screen ends whatever is still running.
    return () => {
      cancelled = true
      observer.disconnect()
      subs.forEach((s) => s.dispose())
      stopStream?.()
      const client = activeClient
      if (exited === null && client && termId) void market.termClose(client, termId).catch(() => {})
      term.dispose()
    }
  }, [store, backend?.id, step, method?.id])

  const motor = backend?.name ? isolado(backend.name) : t('extensoes.terminal.motor')
  const title = step === 'node' ? t('extensoes.motor.adicionarNode') : step === 'install' ? t('extensoes.terminal.instalar', { motor }) : (method?.name ?? t('extensoes.terminal.entrar', { motor }))
  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ScreenHeader title={title} onBack={back} />
      {step === 'login' ? <Text size="caption" tone="secondary">{t(backend?.id === 'antigravity' ? 'extensoes.terminal.dicaAntigravity' : backend?.id === 'opencode' ? 'extensoes.terminal.dicaOpenCode' : backend?.id === 'grok' ? 'extensoes.terminal.dicaGrok' : backend?.id === 'cursor' ? 'extensoes.terminal.dicaCursor' : 'extensoes.terminal.dicaLogin')}</Text> : null}
      {/* A saída do terminal é código: da esquerda para a direita, também em árabe. */}
      <div className="setup-terminal bidi-isolado" dir="ltr">
        <div ref={surface} className="setup-terminal-surface" />
      </div>
      <HStack gap={8} className="setup-terminal-status">
        {error ? (
          <Text tone="danger" lines={2}>{error}</Text>
        ) : exitCode !== null ? (
          <>
            <Icon name={exitCode === 0 ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} color={exitCode === 0 ? 'var(--success)' : 'var(--warning)'} />
            <Text tone="secondary" lines={2}>{exitCode === 0 ? t('extensoes.terminal.terminou') : t('extensoes.terminal.terminouErro', { codigo: exitCode })}</Text>
          </>
        ) : (
          <>
            <ThinkingOrb size={14} color="var(--context-ink-2, var(--ink-2))" />
            <Text tone="secondary" lines={2}>{t('extensoes.terminal.rodando', { computador: isolado(store.hostName) })}</Text>
          </>
        )}
        <Spacer />
        {exitCode !== null ? <Button className="fade-in" onClick={back}>{t('extensoes.terminal.concluir')}</Button> : null}
      </HStack>
      </div>
    </SheetPage>
  )
}
