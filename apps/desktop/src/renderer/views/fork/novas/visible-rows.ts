import { useEffect, useState, type RefObject } from 'react'

// D432: as linhas da conversa à vista, entre o cabeçalho e o campo, por IntersectionObserver: nada é medido a cada
// rolagem. Reobserva quando as linhas mudam (`key`) e quando a altura do campo ou do cabeçalho muda (R1-03 do #511:
// o --composer-height é escrito no estilo de um ancestral; um MutationObserver nele recria as margens).
const px = (el: Element, name: string, fallback: number) => parseFloat(getComputedStyle(el).getPropertyValue(name)) || fallback

export function useVisibleRows(scroller: RefObject<HTMLElement | null>, key: string): ReadonlySet<string> {
  const [visible, setVisible] = useState<ReadonlySet<string>>(new Set())
  const [margin, setMargin] = useState<string | null>(null)
  useEffect(() => {
    const root = scroller.current
    if (!root || typeof MutationObserver === 'undefined') return
    const read = () => setMargin(`-${px(root, '--chat-header-height', 96)}px 0px -${px(root, '--composer-height', 70)}px 0px`)
    read()
    // O ThreadView escreve --composer-height no estilo do `.thread-view` (ResizeObserver do campo).
    const owner = root.closest<HTMLElement>('.thread-view')
    const observer = new MutationObserver(read)
    if (owner) observer.observe(owner, { attributes: true, attributeFilter: ['style'] })
    return () => observer.disconnect()
  }, [scroller])
  useEffect(() => {
    const root = scroller.current
    if (!root || margin === null || typeof IntersectionObserver === 'undefined') return
    const shown = new Set<string>()
    const observer = new IntersectionObserver((changes) => {
      for (const c of changes) {
        const id = (c.target as HTMLElement).dataset.chatItem
        if (!id) continue
        if (c.isIntersecting) shown.add(id)
        else shown.delete(id)
      }
      setVisible(new Set(shown))
    }, { root, rootMargin: margin })
    for (const row of root.querySelectorAll<HTMLElement>('[data-chat-item]')) observer.observe(row)
    return () => observer.disconnect()
  }, [scroller, key, margin])
  return visible
}
