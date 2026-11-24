// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.texto': "Installé seulement sur cet ordinateur : s’il se met en veille ou s’éteint, les bots, les routines et le navigateur s’arrêtent. Pour rester toujours allumé, connectez votre propre VPS ou envisagez le Cloud quand il sera disponible.",
}
