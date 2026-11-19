// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useRef, useState, type ReactNode, type Ref } from 'react'
import { IconButton } from '../../components/Controls'
import { Sheet, usePresence, type PortalLanguage } from '../../components/Overlay'
import { Icon } from '../../components/Icon'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { ThinkingOrb } from '../../components/ThinkingOrb'
import { useT } from '../../lib/fork-i18n/estado'
import { arquivoDaMarca } from './marcas'
import { CascaVazia } from '../../components/toques-mexerica/Toques'
import './marketplace.css'

// Building blocks the Marketplace's screens share (kit MarketplaceView.swift "Pieces").

export function MarketSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="market-section">
      <Text as="div" size="headline" tone="text" className="market-section-title">{title}</Text>
      {children}
    </section>
  )
}

/** Two columns on wide screens, one on a narrow one. */
export function ItemGrid({ children }: { children: ReactNode }) {
  return <div className="item-grid">{children}</div>
}

export function MarketRow({ title, subtitle, added, busy = false, addLabel, icon, badges, onAdd, onOpen }: {
  title: string
  subtitle: string
  added: boolean
  busy?: boolean
  addLabel?: string
  icon: ReactNode
  /** Selos depois do título (`MarketBadge`). */
  badges?: ReactNode
  onAdd: () => void
  /** The whole row opens the item's details; without it the row does what its button does. */
  onOpen?: () => void
}) {
  const t = useT()
  return (
    <ItemRow
      title={title}
      subtitle={subtitle}
      icon={icon}
      badges={badges}
      onOpen={onOpen ?? (added || busy ? undefined : onAdd)}
      trailing={busy ? (
        <HStack as="span" align="stretch" justify="center" className="market-row-status">
          <ThinkingOrb size={18} color="var(--context-ink-2, var(--ink-2))" />
        </HStack>
      ) : added ? (
        <HStack as="span" align="stretch" justify="center" className="market-row-status market-row-added" aria-label={t('extensoes.pecas.adicionado')} title={t('extensoes.pecas.adicionado')}>
          <Icon name="checkmark" size={11} weight="semibold" />
        </HStack>
      ) : (
        <button
          className="capsule-action"
          onClick={(e) => {
            e.stopPropagation()
            onAdd()
          }}
        >
          {addLabel ?? t('extensoes.adicionar')}
        </button>
      )}
    />
  )
}

/** An `ItemGrid` row: icon, title and subtitle, then `trailing`; `onOpen` makes the whole row a button. */
export function ItemRow({ title, subtitle, subtitleCodigo = false, icon, badges, trailing, onOpen }: {
  title: string
  subtitle: string
  /** Subtítulo que é código (comando, URL, caminho): da esquerda para a direita e isolado, em qualquer idioma (R2-01 do #577). */
  subtitleCodigo?: boolean
  icon: ReactNode
  badges?: ReactNode
  /** Buttons here stop the click so it doesn't also open the row. */
  trailing?: ReactNode
  onOpen?: () => void
}) {
  return (
    <div
      className={`market-row ${onOpen ? 'openable' : ''}`}
      role={onOpen ? 'button' : undefined}
      tabIndex={onOpen ? 0 : undefined}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (onOpen && e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault()
          onOpen()
        }
      }}
    >
      <div className="market-row-icon">{icon}</div>
      <VStack gap={2} grow>
        {badges ? (
          <HStack gap={6} shrink>
            <Text size="body" weight="medium" tone="text" truncate dir="auto" title={title}>{title}</Text>
            {badges}
          </HStack>
        ) : (
          <Text size="body" weight="medium" tone="text" truncate dir="auto" title={title}>{title}</Text>
        )}
        {subtitleCodigo
          ? <Text size="subheadline" tone="secondary" truncate className="bidi-isolado" dir="ltr" title={subtitle}>{subtitle}</Text>
          : <Text size="subheadline" tone="secondary" truncate dir="auto" title={subtitle}>{subtitle}</Text>}
      </VStack>
      {trailing}
    </div>
  )
}

/** O campo de busca de Extensões; Enter chama `onSubmit` (a busca de conectores só roda aí). */
export function MarketSearch({ value, onChange, placeholder, onSubmit, className }: {
  value: string
  onChange: (value: string) => void
  placeholder: string
  onSubmit?: (value: string) => void
  className?: string
}) {
  const t = useT()
  return (
    <div className={`market-search${className ? ` ${className}` : ''}`}>
      <Icon name="magnifyingglass" size={13} color="var(--context-ink-2, var(--ink-2))" />
      <input
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        dir={value ? 'auto' : undefined}
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.nativeEvent.isComposing) onSubmit?.(value.trim())
        }}
      />
      {value ? (
        <button
          aria-label={t('extensoes.pecas.limparBusca')}
          title={t('extensoes.pecas.limparBusca')}
          className="market-search-clear"
          onClick={() => {
            onChange('')
            onSubmit?.('')
          }}
        >
          <Icon name="xmark.circle.fill" size={13} />
        </button>
      ) : null}
    </div>
  )
}

/** Um selo curto ao lado do título de uma linha ("Conectado", "Instant"). */
export function MarketBadge({ label, icon, tone = 'secondary', title }: { label: string; icon?: string; tone?: 'secondary' | 'added'; title?: string }) {
  return (
    <HStack as="span" gap={3} className={`market-badge ${tone}`} title={title}>
      {icon ? <Icon name={icon} size={9} weight="semibold" /> : null}
      <Text size="caption2" weight="semibold">{label}</Text>
    </HStack>
  )
}

/** A quiet "nothing here" message (replaces `ContentUnavailableView`), with an optional action. */
export function EmptyState({ icon, title, detail, action, toque = false }: { icon: string; title: string; detail: string; action?: ReactNode; toque?: boolean }) {
  return (
    <VStack align="center" gap={8} className="market-empty">
      {toque ? <CascaVazia fallback={<Icon name={icon} size={28} color="var(--text-tertiary, var(--ink-3))" />} /> : <Icon name={icon} size={28} color="var(--text-tertiary, var(--ink-3))" />}
      <Text size="compactBody" weight="semibold" tone="text">{title}</Text>
      <Text size="compactSecondary" tone="secondary">{detail}</Text>
      {action}
    </VStack>
  )
}

/** Placeholder rows while a list loads, shaped like the real ones. */
export function SkeletonGrid({ gridRef }: { gridRef?: Ref<HTMLDivElement> } = {}) {
  return (
    <div ref={gridRef} className="item-grid skeleton">
      {[0, 1, 2, 3].map((i) => (
        <HStack key={i} gap={14} className="skeleton-row">
          <div className="skeleton-icon" />
          <VStack gap={8}>
            <div className="skeleton-title" />
            <div className="skeleton-subtitle" />
          </VStack>
        </HStack>
      ))}
    </div>
  )
}

/**
 * O ícone do serviço: a marca dele, em arquivo local, quando o catálogo traz a `marca`; sem id ou sem arquivo,
 * o globo genérico. Nunca busca nada na rede nem revela o domínio do serviço a provedores de favicon.
 */
export function ServiceLogo({ size = 46, marca: chave }: { website?: string | null; site?: string | null; name: string; registryName?: string | null; marca?: string | null; size?: number }) {
  const marca = arquivoDaMarca(chave)
  return (
    <div aria-hidden className={`market-logo service-logo${marca ? ' service-logo-image' : ''}`} style={{ width: size, height: size, borderRadius: size * 0.26 }}>
      {marca ? <img className="service-logo-marca" src={marca} alt="" draggable={false} /> : <Icon name="globe" size={size * 0.5} />}
    </div>
  )
}

export function TileIcon({ name }: { name: string }) {
  return (
    <div className="market-tile">
      <Icon name={name} size={18} weight="medium" />
    </div>
  )
}

export function SkillGlyph() {
  return (
    <div className="market-tile">
      <Icon name="book.pages" size={13} />
    </div>
  )
}

/** Text that opens a web page (replaces the system `Link`). */
export function WebLink({ title, url, className }: { title: string; url: string; className?: string }) {
  return (
    <button className={`plain-link${className ? ` ${className}` : ''}`} title={url} onClick={() => window.codync.app.openExternal(url)}>
      {title}
    </button>
  )
}

/** A full screen's top bar (kit `ScreenHeader`): back on the leading side, centered title, trailing. */
export function ScreenHeader({ title, onBack, trailing }: { title: string; onBack: () => void; trailing?: ReactNode }) {
  const t = useT()
  return (
    <HStack gap={6} className="market-screen-header">
      <Text as="div" size="compactBody" weight="semibold" tone="text" truncate dir="auto" className="market-screen-title">{title}</Text>
      <IconButton title={t('extensoes.pecas.voltar')} icon="chevron.left" onClick={onBack} />
      <Spacer min={8} />
      {trailing}
    </HStack>
  )
}

/**
 * A screen pushed over another inside one modal: the base slides out leading as `top` slides
 * in from the trailing edge (kit's `.move(edge:)` + opacity transitions), and back.
 */
export function SlideStack({ base, top }: { base: ReactNode; top: ReactNode | null }) {
  const { mounted, shown } = usePresence(top !== null)
  const last = useRef(top)
  if (top !== null) last.current = top
  return (
    <div className="market-stack">
      <div className={`market-layer ${shown ? 'off-leading' : ''}`} inert={shown}>{base}</div>
      {mounted ? <div className={`market-layer ${shown ? '' : 'off-trailing'}`}>{top ?? last.current}</div> : null}
    </div>
  )
}

/** Plain text entry for card rows: one line, a secret, or a growing multi-line box. */
export function Field({ value, onChange, placeholder, secret = false, lines, mono = false, ltr = false, label }: {
  value: string
  onChange: (v: string) => void
  placeholder?: string
  secret?: boolean
  /** Multi-line, growing between these line counts (`axis: .vertical` + `.lineLimit`). */
  lines?: [number, number]
  mono?: boolean
  /** Dado técnico (site, usuário, e-mail): da esquerda para a direita, sem a fonte monoespaçada. */
  ltr?: boolean
  label?: string
}) {
  const className = `market-field${mono ? ' mono' : ''}`
  // Código (comando, URL, config, cabeçalhos) e segredo vão da esquerda para a direita, também em árabe; o resto segue
  // a direção do próprio texto.
  // Vazio, o campo herda a direção da tela (com dir="auto", o placeholder em árabe ficaria da esquerda para a direita).
  const dir = mono || ltr || secret ? 'ltr' : value ? 'auto' : undefined
  if (lines) {
    return (
      <textarea
        className={className}
        aria-label={label ?? placeholder}
        value={value}
        placeholder={placeholder}
        dir={dir}
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        style={{ minHeight: `${lines[0] * 1.3}em`, maxHeight: `${lines[1] * 1.3}em` }}
      />
    )
  }
  return (
    <input
      className={className}
      aria-label={label ?? placeholder}
      type={secret ? 'password' : 'text'}
      value={value}
      placeholder={placeholder}
      dir={dir}
      spellCheck={false}
      autoComplete="off"
      onChange={(e) => onChange(e.target.value)}
    />
  )
}

/** A danger-colored error line in its own card. */
export function ErrorText({ text }: { text: string }) {
  return <Text tone="danger" className="selectable">{text}</Text>
}

/** Values with something typed in. */
export const filled = (values: Record<string, string>) => Object.fromEntries(Object.entries(values).filter(([, v]) => v !== ''))

/** The window's height, following resizes. */
export function useWindowHeight() {
  const [height, setHeight] = useState(window.innerHeight)
  useEffect(() => {
    const onResize = () => setHeight(window.innerHeight)
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  return height
}

/**
 * A Marketplace form in its own modal card (`.codyncSheet`), as tall as its content up to
 * `height`; `fixed` keeps it at `height` for content that fills the card (a terminal).
 */
export function FormSheet({ open, onClose, width = 520, height = 600, fixed = false, children, language }: { open: boolean; onClose: () => void; width?: number; height?: number; fixed?: boolean; children: ReactNode; language?: PortalLanguage }) {
  const windowHeight = useWindowHeight()
  const cap = Math.min(height, windowHeight - 76)
  return (
    <Sheet open={open} onClose={onClose} width={width} height={fixed ? cap : undefined} maxHeight={cap} language={language}>
      {children}
    </Sheet>
  )
}
