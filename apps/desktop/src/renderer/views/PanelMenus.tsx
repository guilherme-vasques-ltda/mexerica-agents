import { useEffect, useRef, useState, type ReactNode } from 'react'
import { superficieIdioma, useIdioma, useT } from '../lib/fork-i18n/estado'
import { Spinner } from '../components/Controls'
import { Icon } from '../components/Icon'
import { useEscape, type MenuItem } from '../components/Overlay'
import { interactive } from '../lib/fork-modal-stack'
import { HStack, IconSlot } from '../components/Stack'
import { Text } from '../components/Text'
import type { Link } from '@shared/analytics'
import { showsAccountControls } from '@shared/fork-features'
import { account, useAccount } from '../store/account'
import { openLink, useScreenViewed } from '../store/analytics'
import googleLogo from '../assets/google.svg'

// The chat window's floating panels: the account menu (with its profile avatar) and the roster's
// right-click actions, both built on the same keyboard-driven rows.

export function ProfileAvatar() {
  const { user, isBusy } = useAccount()
  return (
    <span className="profile-avatar" aria-hidden>
      {user?.avatarURL ? (
        <img src={user.avatarURL} alt="" className="profile-avatar-image" />
      ) : (
        <Icon name="person.fill" size={15} weight="medium" color="var(--context-ink-2, var(--ink-2))" />
      )}
      {isBusy ? (
        <HStack as="span" justify="center" className="profile-avatar-busy">
          <Spinner size={14} />
        </HStack>
      ) : null}
    </span>
  )
}

interface PanelItem {
  title: string
  icon: string
  detail?: string | null
  /** A bundled image in place of the symbol (the Google logo). */
  image?: string
  chevron?: boolean
  disabled?: boolean
  destructive?: boolean
  action: () => void
}

/** The account menu: a window-local floating surface (no system menu styling). */
export function AccountPanelLayer({ compact, approvals, onDismiss, onUsage, onSettings }: {
  compact: boolean
  approvals: number
  onDismiss: () => void
  onUsage: () => void
  onSettings: () => void
}) {
  const [page, setPage] = useState<'main' | 'support'>('main')
  const { user, isBusy, errorMessage, isExitUnresolved, canAct, socialProviders } = useAccount()
  useScreenViewed('account')
  // ajustes-tela-01: na pilha do Esc, acima da tela que estiver por baixo (Ajustes, editor do bot): Esc fecha só o menu.
  const layer = useRef<HTMLDivElement>(null)
  useEscape(true, onDismiss, layer)
  const signedIn = user !== null
  const t = useT()
  const superficie = superficieIdioma(useIdioma())
  // D185: recuperação transitória usa retry ou nova entrada, sem confirmação restrita.
  const auth: PanelItem[] = isExitUnresolved
    ? [{ title: t('navegacao.janela.conta.tentarOuEntrar'), icon: 'exclamationmark.triangle', chevron: true, action: onSettings }]
    : signedIn
    ? [
        { title: t(isBusy ? 'navegacao.janela.conta.aguarde' : 'navegacao.janela.conta.sair'), icon: 'rectangle.portrait.and.arrow.right', disabled: !canAct, action: () => { onDismiss(); void account.requestSignOut() } },
      ]
    : [
        ...(socialProviders.includes('apple') ? [{ title: t('navegacao.janela.conta.apple'), icon: 'apple.logo', disabled: !canAct, action: () => { onDismiss(); void account.signIn('apple') } }] : []),
        ...(socialProviders.includes('google') ? [{ title: t('navegacao.janela.conta.google'), icon: 'person.crop.circle.badge.plus', image: googleLogo, disabled: !canAct, action: () => { onDismiss(); void account.signIn('google') } }] : []),
      ]
  const open = (url: string, link: Link) => {
    onDismiss()
    openLink(url, link)
  }
  const items: PanelItem[] =
    page === 'support'
      ? [
          { title: t('navegacao.janela.conta.suporte'), icon: 'chevron.left', action: () => setPage('main') },
          { title: t('navegacao.janela.conta.ajuda'), icon: 'book', chevron: true, action: () => open('https://example.invalid/mexerica-agents#readme', 'docs') },
          { title: t('navegacao.janela.conta.problema'), icon: 'bubble.left', chevron: true, action: () => open('https://example.invalid/mexerica-agents/issues', 'issues') },
        ]
      : [
          { title: t('navegacao.janela.conta.uso'), icon: 'gauge.with.dots.needle.33percent', chevron: true, action: onUsage },
          // Sem "Get Codync for mobile": o app do iPhone do upstream não pareia com o Donnabot (D48, mobile depois).
          { title: t('navegacao.janela.conta.suporte'), icon: 'book.closed', chevron: true, action: () => setPage('support') },
          { title: t('navegacao.janela.conta.ajustes'), icon: 'gearshape', detail: approvals > 0 ? String(approvals) : null, action: onSettings },
          ...(showsAccountControls() ? auth : []),
        ]
  return (
    <div ref={layer} className="context-layer idioma-superficie" {...superficie} onMouseDown={(e) => e.target === e.currentTarget && onDismiss()}>
      <div className={`account-panel ${compact ? 'compact' : ''}`} style={{ width: Math.min(260, window.innerWidth - 32) }}>
        <PanelRows
          items={items}
          onDismiss={onDismiss}
          after={(i) =>
            showsAccountControls() && page === 'main' && i === 2 ? (
              <>
                <div className="panel-divider" />
                <div className="panel-identity">
                  <Icon name="person.crop.circle" size={15} />
                  <Text size="callout" truncate grow>{user?.email ? <bdi dir="ltr">{user.email}</bdi> : t('navegacao.janela.conta.conta')}</Text>
                  {signedIn ? null : <Text size="caption">{t('navegacao.janela.conta.naoConectado')}</Text>}
                </div>
              </>
            ) : page === 'support' && i === 0 ? (
              <div className="panel-divider" />
            ) : null
          }
        />
        {page === 'main' && errorMessage && !isExitUnresolved ? (
          <Text as="div" size="callout" tone="attention" className="panel-error">{errorMessage}</Text>
        ) : null}
      </div>
    </div>
  )
}

function PanelRows({ items, onDismiss, after, dismissOnActivate = false }: { items: PanelItem[]; onDismiss: () => void; after?: (i: number) => ReactNode; dismissOnActivate?: boolean }) {
  const [highlighted, setHighlighted] = useState(0)
  const panel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || !interactive(panel.current)) return
      if (e.key === 'Escape') {
        e.preventDefault()
        onDismiss()
      } else if (e.key === 'ArrowDown' || (e.key === 'Tab' && !e.shiftKey)) {
        e.preventDefault()
        setHighlighted((h) => (h + 1) % items.length)
      } else if (e.key === 'ArrowUp' || (e.key === 'Tab' && e.shiftKey)) {
        e.preventDefault()
        setHighlighted((h) => (h - 1 + items.length) % items.length)
      } else if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        const item = items[highlighted]
        if (item && !item.disabled) {
          if (dismissOnActivate) onDismiss()
          item.action()
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [items, highlighted, onDismiss, dismissOnActivate])
  return (
    <div ref={panel} className="vstack" style={{ gap: 0 }}>
      {items.map((item, i) => (
        <div key={item.title}>
          <button
            className={`panel-row ${highlighted === i ? 'highlight' : ''} ${item.destructive ? 'destructive' : ''}`}
            disabled={item.disabled}
            onMouseEnter={() => setHighlighted(i)}
            onClick={() => {
              if (dismissOnActivate) onDismiss()
              item.action()
            }}
          >
            <IconSlot width={20}>
              {item.image ? <img src={item.image} alt="" width={15} height={15} /> : <span className="panel-row-icon"><Icon name={item.icon} size={15} /></span>}
            </IconSlot>
            <Text size="body" truncate grow>{item.title}</Text>
            {item.detail ? <Text size="body" tone="secondary">{item.detail}</Text> : null}
            {item.chevron ? <Icon name="chevron.right" size={11} color="var(--context-ink-2, var(--ink-2))" /> : null}
          </button>
          {after?.(i)}
        </div>
      ))}
    </div>
  )
}

/** The roster's right-click actions, shared by pointer and keyboard. */
export function DesktopActionMenu({ items, onDismiss, style }: { items: MenuItem[]; onDismiss: () => void; style: React.CSSProperties }) {
  const rows = items.map<PanelItem>((m) => ({ title: m.title, icon: m.icon ?? 'circle', destructive: m.destructive, action: m.action }))
  return (
    <div className="action-menu" style={style}>
      <PanelRows
        items={rows}
        onDismiss={onDismiss}
        dismissOnActivate
        after={(i) => (items[i + 1]?.divider ? <div className="panel-divider" /> : null)}
      />
    </div>
  )
}
