/** Atalhos da janela; independem da montagem/visibilidade da barra lateral. */
export function spaceShortcut(e: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'repeat' | 'isComposing' | 'defaultPrevented'>, mac: boolean, ids: readonly string[], activeId: string | null): { open: true } | { id: string } | null {
  if (e.defaultPrevented || e.repeat || e.isComposing || e.altKey) return null
  const command = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
  if (command && e.shiftKey && e.key.toLowerCase() === 'k') return { open: true }
  if (e.ctrlKey && !e.metaKey && e.key === 'Tab' && ids.length) {
    const index = Math.max(0, ids.indexOf(activeId ?? ''))
    return { id: ids[(index + (e.shiftKey ? -1 : 1) + ids.length) % ids.length]! }
  }
  if (command && !e.shiftKey && /^[1-9]$/.test(e.key)) {
    const id = ids[Number(e.key) - 1]
    return id ? { id } : null
  }
  return null
}
