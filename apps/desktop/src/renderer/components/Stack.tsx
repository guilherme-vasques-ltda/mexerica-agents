// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { ComponentPropsWithoutRef, CSSProperties, ReactNode } from 'react'
import type { Tone } from './Text'

type StackProps = {
  as?: 'div' | 'span'
  gap?: number
  align?: CSSProperties['alignItems']
  justify?: CSSProperties['justifyContent']
  wrap?: boolean
  /** Takes the free space along the parent stack (and may shrink below its content). */
  grow?: boolean
  /** May shrink below its content inside the parent stack, so long text inside can truncate. */
  shrink?: boolean
  className?: string
  style?: CSSProperties
  children?: ReactNode
} & Omit<ComponentPropsWithoutRef<'div'>, 'className' | 'style' | 'children'>

function Stack({ direction, as = 'div', gap, align, justify, wrap = false, grow = false, shrink = false, className, style, children, ...rest }: StackProps & { direction: 'row' | 'column' }) {
  // The element only changes the tag; the props are a div's either way.
  const Tag = as as 'div'
  return (
    <Tag
      className={`${direction === 'row' ? 'hstack' : 'vstack'}${className ? ` ${className}` : ''}`}
      style={{ gap, alignItems: align, justifyContent: justify, flexWrap: wrap ? 'wrap' : undefined, flex: grow ? 1 : undefined, minWidth: grow || shrink ? 0 : undefined, ...style }}
      {...rest}
    >
      {children}
    </Tag>
  )
}

/** Children side by side, centered vertically unless `align` says otherwise (SwiftUI's `HStack`). */
export function HStack({ align = 'center', ...props }: StackProps) {
  return <Stack direction="row" align={align} {...props} />
}

/** Children top to bottom, stretched across (SwiftUI's `VStack`). */
export function VStack(props: StackProps) {
  return <Stack direction="column" {...props} />
}

/** Pushes its neighbors apart in a stack, keeping at least `min` between them (SwiftUI's `Spacer`). */
export function Spacer({ min }: { min?: number }) {
  return <span className="spacer" style={min ? { minWidth: min, minHeight: min } : undefined} />
}

// IconSlot uses generic tints, independently of Text's contextual semantic foregrounds.
const iconToneColor: Record<Tone, string> = {
  text: 'var(--text-primary, var(--ink))',
  secondary: 'var(--context-ink-2, var(--ink-2))',
  tertiary: 'var(--text-tertiary, var(--ink-3))',
  accent: 'var(--accent)',
  attention: 'var(--attention)',
  danger: 'var(--danger)',
  added: 'var(--success)',
  removed: 'var(--diff-removal-ink, var(--danger))',
}

/** A fixed-width column that centers an icon, so the labels beside it line up (`width` in points). */
export function IconSlot({ width, tone, className, children, ...rest }: { width: number; tone?: Tone; className?: string; children: ReactNode } & Omit<ComponentPropsWithoutRef<'span'>, 'className' | 'children'>) {
  return (
    <HStack as="span" align="stretch" justify="center" className={`icon-slot${className ? ` ${className}` : ''}`} style={{ width, color: tone && iconToneColor[tone] }} {...rest}>
      {children}
    </HStack>
  )
}
