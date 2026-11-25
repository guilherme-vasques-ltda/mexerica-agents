// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.texto': "Installed only on this computer: if it sleeps or shuts down, bots, routines, and the browser stop. To stay always on, connect your own VPS or consider Cloud when it's available.",
  'avisoLocal.rc2HubLocal': 'Local Hub on: without an authenticated pair between the app and the Hub, there is no guarantee that something only happens after your click.',
}
