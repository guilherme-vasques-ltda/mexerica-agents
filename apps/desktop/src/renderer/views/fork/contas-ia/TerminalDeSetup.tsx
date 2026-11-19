import { FitAddon } from '@xterm/addon-fit'
import { Terminal } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'
import { useEffect, useRef } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { ThinkingOrb } from '../../../components/ThinkingOrb'
import { useModel } from '../../../lib/observable'
import { ScreenHeader } from '../../marketplace/pieces'
import { clockText, errorMessage, expiresAtMs, isFinal } from './regras'
import type { SetupSession } from './setup-session'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'

/** Caracteres que o Hub aceita num código de autorização. */
const CODE_CHAR = /^[A-Za-z0-9\-_.~#]$/
const MAX_LINE = 2048

/**
 * O terminal alternativo do login: anexado à operação do Hub (`open_terminal`, `read`,
 * `terminal_input`, `terminal_resize`, `cancel`), nunca a um shell. Mostra a apresentação que
 * o Hub permite, filtrada de novo aqui, sem área de transferência nem links. A digitação fica
 * numa linha local mascarada e só o código inteiro vai ao Hub, no Enter.
 */
export function TerminalDeSetup({ session, hostName }: { session: SetupSession; hostName: string }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  useModel(session)
  const surface = useRef<HTMLDivElement>(null)
  const terminal = useRef<Terminal | null>(null)
  const writable = session.terminalWritable
  const writableRef = useRef(writable)
  writableRef.current = writable

  useEffect(() => {
    const el = surface.current
    if (!el) return
    // Root colors are already resolved; the Sheet has a different local ground.
    const css = getComputedStyle(document.documentElement)
    const scale = parseFloat(css.getPropertyValue('--scale')) || 1
    const term = new Terminal({
      fontFamily: css.getPropertyValue('--font-mono').trim() || 'monospace',
      fontSize: 12 * scale,
      cursorBlink: false,
      allowTransparency: true,
      disableStdin: !writableRef.current,
      scrollback: 500,
      theme: {
        background: css.getPropertyValue('--canvas').trim(),
        foreground: css.getPropertyValue('--ink').trim(),
        cursor: css.getPropertyValue('--ink').trim(),
        selectionBackground: `rgba(${css.getPropertyValue('--ink-rgb').trim()}, 0.2)`,
      },
    })
    terminal.current = term
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(el)
    fit.fit()
    let line = ''
    const off = session.onTerminal((text) => {
      term.reset()
      if (text) term.write(text)
      if (line) term.write('•'.repeat(line.length))
    })
    const input = term.onData((data) => {
      // Teclas especiais (setas, F1…) chegam como sequência inteira: nada delas vai ao Hub.
      if (!writableRef.current || data.startsWith('\u001b')) return
      for (const ch of data) {
        if (ch === '\r' || ch === '\n') {
          if (!line) continue
          const code = line
          line = ''
          term.write('\r\n')
          void session.submitTerminalLine(code)
        } else if (ch === '\u007f' || ch === '\b') {
          if (!line) continue
          line = line.slice(0, -1)
          term.write('\b \b')
        } else if (ch === '\u0015') {
          term.write('\b \b'.repeat(line.length))
          line = ''
        } else if (CODE_CHAR.test(ch) && line.length < MAX_LINE) {
          line += ch
          term.write('•')
        }
      }
    })
    const resize = term.onResize(({ cols, rows }) => session.resize(cols, rows))
    session.resize(term.cols, term.rows)
    const observer = new ResizeObserver(() => fit.fit())
    observer.observe(el)
    if (writableRef.current) term.focus()
    return () => {
      off()
      input.dispose()
      resize.dispose()
      observer.disconnect()
      line = ''
      terminal.current = null
      term.dispose()
    }
  }, [session])

  useEffect(() => {
    const term = terminal.current
    if (!term) return
    term.options.disableStdin = !writable
    if (writable) term.focus()
  }, [writable])

  const op = session.operation
  const ended = !op || isFinal(op.phase)
  const expires = op?.expiresAt ? expiresAtMs(op.expiresAt) : null
  const note = ended ? textos.terminalEncerrado
    : session.overdue ? textos.terminalVencido
    : session.submitting || op?.phase === 'verifying' ? textos.faseVerificando
    : !op?.terminal?.available ? textos.terminalIndisponivel
    : textos.terminalAtivo(hostName)

  return (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ScreenHeader title={textos.terminalTitulo} onBack={() => session.closeTerminal()} />
      <VStack gap={2} className="contas-terminal-aviso">
        <HStack gap={6}>
          <Icon name="lock.shield" size={11} color="var(--context-ink-2, var(--ink-2))" />
          <Text size="caption" tone="secondary">{textos.terminalAviso}</Text>
        </HStack>
      </VStack>
      <div className="setup-terminal" dir="ltr" role="group" aria-label={textos.terminalTitulo}>
        <div ref={surface} className="setup-terminal-surface" />
      </div>
      <HStack gap={8} className="setup-terminal-status contas-terminal-status">
        {ended || session.overdue ? (
          <Icon name="exclamationmark.circle.fill" size={10} color="var(--warning)" />
        ) : (
          <ThinkingOrb size={14} color="var(--context-ink-2, var(--ink-2))" />
        )}
        <VStack gap={1} shrink>
          <Text tone="secondary" lines={2} role="status" aria-live="polite">{session.cancelling && !ended ? textos.faseCancelando : note}</Text>
          {expires && !ended ? <Text size="caption" tone={session.overdue ? 'attention' : 'tertiary'}>{session.overdue ? textos.venceu : textos.valeAte(clockText(expires, idioma))}</Text> : null}
          {session.error ? <Text size="caption" tone="danger" role="alert">{errorMessage(session.error, idioma)}</Text> : null}
        </VStack>
        <Spacer />
        {!ended ? (
          <Button kind="secondary" disabled={session.cancelling} onClick={() => void session.cancel()}>{textos.terminalCancelar}</Button>
        ) : null}
        <Button kind={ended ? 'primary' : 'secondary'} onClick={() => session.closeTerminal()}>{textos.terminalVoltar}</Button>
      </HStack>
    </div></SheetPage>
  )
}
