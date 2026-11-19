import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/idioma.ts'

// DON-903: idioma (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'idioma.nome': 'English',
  'idioma.passo.titulo1': 'All your bots,',
  'idioma.passo.titulo2': 'in one place.',
  'idioma.passo.subtitulo': 'Berga runs your team of bots in Mexerica and lets you know when something needs you.',
  'idioma.passo.escolha': 'Choose your language',
  'idioma.passo.sugerido': "Suggested by your Mac's language",
  'idioma.passo.continuar': 'Continue',
  'idioma.passo.rodape': 'You can change this later in Settings → General.',
  'idioma.passo.etapa': 'Step {atual} of {total}',
  'idioma.aviso.migracao': 'Translation is in progress: account sign-in and most of the app are still in Portuguese.',
  'idioma.aviso.bots': 'Bots still reply in Portuguese.',
  'idioma.ajuste.secao': 'Language',
  'idioma.ajuste.rotulo': 'App language',
}
