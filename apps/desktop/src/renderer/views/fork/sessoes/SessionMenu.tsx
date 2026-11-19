import { useLayoutEffect } from 'react'
import { AnchoredMenu } from '../../../components/Overlay'

/** Apenas os menus D545: foco inicial, navegação e retorno à linha/aba de origem. */
export function SessionMenu(props: Parameters<typeof AnchoredMenu>[0]) {
  useLayoutEffect(() => {
    if (!props.open) return
    const origin = document.activeElement
    return () => {
      if (origin instanceof HTMLElement && origin.isConnected && !origin.closest('[inert]')) origin.focus({ preventScroll: true })
    }
  }, [props.open])
  return <AnchoredMenu {...props} focusMenu />
}
