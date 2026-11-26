// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.rc2Titulo': 'Mexerica RC 2',
  'avisoLocal.rc2Inativa': 'C’est une version de test. Le Hub local n’est pas encore activé.',
  'avisoLocal.rc2Verificacao': 'Impossible de vérifier cette installation. Le Hub local n’a pas démarré.',
  'avisoLocal.texto': "Installé seulement sur cet ordinateur : s’il se met en veille ou s’éteint, les bots, les routines et le navigateur s’arrêtent. Pour rester toujours allumé, connectez votre propre VPS ou envisagez le Cloud quand il sera disponible.",
  'avisoLocal.rc2HubLocal': "Hub local actif : sans paire authentifiée entre l’app et le Hub, rien ne garantit qu’une action n’arrive qu’après votre clic.",
}
