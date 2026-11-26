// Aviso de instalación solo en este ordenador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.rc2Titulo': 'Mexerica RC 2',
  'avisoLocal.rc2Inativa': 'Esta es una versión de prueba. El Hub local aún no está habilitado.',
  'avisoLocal.rc2Verificacao': 'No se pudo verificar esta instalación. El Hub local no se inició.',
  'avisoLocal.texto': 'Instalado solo en este ordenador: si se suspende o se apaga, los bots, las rutinas y el navegador se detienen. Para estar siempre encendido, conecta un VPS propio o considera la Cloud cuando esté disponible.',
  'avisoLocal.rc2HubLocal': 'Hub local encendido: sin un par autenticado entre la app y el Hub, no hay garantía de que algo ocurra solo después de tu clic.',
}
