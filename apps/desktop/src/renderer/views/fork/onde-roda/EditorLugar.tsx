import { useRef, type ReactNode } from 'react'
import { Button } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'

/** Navegação local: fechar conserva o rascunho e não envia intenção ao Hub. */
export function EditorLugar({ title, children }: { title: string; children: ReactNode }) {
  const details = useRef<HTMLDetailsElement>(null)
  const t = useT()
  const close = () => {
    const element = details.current
    if (!element) return
    const restore = element.contains(document.activeElement)
    element.open = false
    if (restore) element.querySelector('summary')?.focus({ preventScroll: true })
  }
  return <details ref={details} className="lugares-editor">
    <summary className="styled-button secondary">{title}</summary>
    <div className="lugares-editor-body">
      {children}
      <Button kind="secondary" onClick={close}>{t('lugares.fecharEdicao')}</Button>
    </div>
  </details>
}
