// Aviso de instalación solo en este ordenador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.texto': 'Instalado solo en este ordenador: si se suspende o se apaga, los bots, las rutinas y el navegador se detienen. Para estar siempre encendido, conecta un VPS propio o considera la Cloud cuando esté disponible.',
}
