import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/idioma.ts'

// DON-903: idioma (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'idioma.nome': 'Español',
  'idioma.passo.titulo1': 'Todos tus bots,',
  'idioma.passo.titulo2': 'en un solo lugar.',
  'idioma.passo.subtitulo': 'Berga organiza tu equipo de bots en Mexerica y te avisa cuando algo requiere tu atención.',
  'idioma.passo.escolha': 'Elige tu idioma',
  'idioma.passo.sugerido': 'Sugerido según el idioma de tu Mac',
  'idioma.passo.continuar': 'Continuar',
  'idioma.passo.rodape': 'Podrás cambiarlo más adelante en Ajustes → General.',
  'idioma.passo.etapa': 'Paso {atual} de {total}',
  'idioma.aviso.migracao': 'La traducción está en curso: el inicio de sesión y la mayor parte de la aplicación todavía están en portugués.',
  'idioma.aviso.bots': 'Los bots siguen respondiendo en portugués.',
  'idioma.ajuste.secao': 'Idioma',
  'idioma.ajuste.rotulo': 'Idioma de la aplicación',
}
