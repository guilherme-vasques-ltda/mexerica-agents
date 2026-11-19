import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/idioma.ts'

// DON-903: idioma (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'idioma.nome': 'Français',
  'idioma.passo.titulo1': 'Tous vos bots,',
  'idioma.passo.titulo2': 'réunis au même endroit.',
  'idioma.passo.subtitulo': 'Berga organise votre équipe de bots dans Mexerica et vous avertit lorsque quelque chose nécessite votre attention.',
  'idioma.passo.escolha': 'Choisissez votre langue',
  'idioma.passo.sugerido': 'Suggéré selon la langue de votre Mac',
  'idioma.passo.continuar': 'Continuer',
  'idioma.passo.rodape': 'Vous pourrez la modifier plus tard dans Réglages → Général.',
  'idioma.passo.etapa': 'Étape {atual} sur {total}',
  'idioma.aviso.migracao': 'La traduction est en cours : la connexion au compte et la majeure partie de l’application sont encore en portugais.',
  'idioma.aviso.bots': 'Les bots continuent de répondre en portugais.',
  'idioma.ajuste.secao': 'Langue',
  'idioma.ajuste.rotulo': 'Langue de l’application',
}
