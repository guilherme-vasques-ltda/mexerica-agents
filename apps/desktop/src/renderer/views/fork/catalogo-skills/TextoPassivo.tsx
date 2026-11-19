import { useLayoutEffect, useRef, type SyntheticEvent } from 'react'
import { MarkdownText } from '../../thread/Markdown'

/** Clique, clique do meio ou Enter num link do texto da skill não fazem nada. */
const bloquear = (e: SyntheticEvent) => {
  if (e.target instanceof Element && e.target.closest('a')) {
    e.preventDefault()
    e.stopPropagation()
  }
}

/**
 * O texto de uma skill é dado não confiável: o Markdown do app (sem HTML) em modo passivo.
 * Links viram texto com o endereço no `title`; nada abre site, arquivo ou programa.
 */
export function TextoPassivo({ texto }: { texto: string }) {
  const caixa = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    for (const link of caixa.current?.querySelectorAll('a') ?? []) {
      link.removeAttribute('href')
      link.tabIndex = -1
      link.classList.add('cs-link-passivo')
    }
  })
  return (
    // Texto de terceiro na direção dele (R1-03 do #579); os blocos de código são LTR no próprio Markdown.
    <div ref={caixa} className="cs-texto-passivo" dir="auto" onClickCapture={bloquear} onAuxClickCapture={bloquear} onMouseDownCapture={bloquear}>
      <MarkdownText text={texto} />
    </div>
  )
}
