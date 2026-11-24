import { useEffect } from 'react'
import { useModel } from '../../lib/observable'
import { useApp } from '../../store/context'

/** Returning from System Settings only checks state; it never requests a grant. */
export function useComputerAccess(observeRestoreNotice: boolean) {
  const app = useApp()
  useModel(observeRestoreNotice && window.codync.platform === 'linux' ? app.local : null)
  const access = useModel(app.computerAccess)
  useEffect(() => {
    const refresh = () => void access.refresh()
    refresh()
    const timer = window.setInterval(refresh, 2_000)
    window.addEventListener('focus', refresh)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener('focus', refresh)
    }
  }, [access])
  return access
}
