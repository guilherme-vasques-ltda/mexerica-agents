import { useLayoutEffect, useRef, useState } from 'react'
import { interactive } from '../../lib/fork-modal-stack'

export function useRoutineCopyFeedback() {
  const [copyId, setCopyId] = useState<string | null>(null)
  const copyRow = useRef<HTMLButtonElement>(null)
  const focus = useRef<{ id: string; origin: Element | null; done: boolean } | null>(null)

  useLayoutEffect(() => {
    if (!copyId) {
      focus.current = null
      return
    }
    const request = focus.current?.id === copyId
      ? focus.current
      : { id: copyId, origin: document.activeElement, done: false }
    focus.current = request
    if (request.done) return
    if (document.activeElement !== request.origin && document.activeElement !== document.body) {
      request.done = true
    } else if (interactive(copyRow.current)) {
      request.done = true
      copyRow.current.focus()
    }
  })

  return { copyId, setCopyId, copyRow }
}
