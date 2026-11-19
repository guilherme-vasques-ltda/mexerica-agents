import { useSyncExternalStore } from 'react'
import { pref } from './prefs'

/** Preferência compartilhada; não depende de nenhuma view nem do carregamento de Aparência. */
export const toquesMexericaPref = pref('appearanceMexericaTouches', true, (value): value is boolean => typeof value === 'boolean')

export function useToquesMexerica() {
  return useSyncExternalStore(toquesMexericaPref.subscribe, toquesMexericaPref.get, toquesMexericaPref.get)
}
