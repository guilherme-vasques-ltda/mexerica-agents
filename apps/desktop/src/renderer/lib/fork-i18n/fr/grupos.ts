import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/grupos.ts'

// Grupos de bots (registro.ts). Chaves completas; docs/guides/idiomas.md.
// Escrito a partir do significado do pt-BR novo (grupos.ts), sem consultar texto anterior.
export const textos: Traducao<typeof fonte> = {
  'grupos.menu.novo': "Créer un groupe",
  'grupos.nomePadrao': "Nouveau groupe",
  'grupos.nome.rotulo': "Nom du groupe",
  'grupos.menu.renomear': "Renommer le groupe",
  'grupos.menu.apagar': "Supprimer le groupe",
  'grupos.menu.recolher': "Réduire le groupe",
  'grupos.menu.mostrar': "Afficher le groupe",
  'grupos.bot.mover': "Placer dans {grupo}",
  'grupos.bot.tirar': "Sortir du groupe",
  'grupos.bot.novoGrupo': "Démarrer un groupe avec {bot}",
  'grupos.foraDeGrupo': "(hors groupe)",
  'grupos.vazio': "Aucun bot dans ce groupe.",
  'grupos.dica': "Faites glisser un bot vers un groupe ou utilisez son menu.",
  'grupos.filtro.todos': "Tous les groupes",
  'grupos.erro.nomeInvalido': "Le nom ne peut pas être vide, dépasser 40 caractères ni contenir de caractères invisibles ou de contrôle.",
  'grupos.erro.nomeUsado': "Un autre groupe de cet espace porte déjà ce nom.",
  'grupos.erro.grupoSumiu': "Ce groupe n’existe plus.",
  'grupos.erro.botSumiu': "Ce bot n’existe plus.",
  'grupos.erro.atualizando': "Les groupes sont en cours de mise à jour. Réessayez dans un instant.",
  'grupos.erro.generico': "Impossible de mettre à jour les groupes. Réessayez.",
  'grupos.contagem.one': "{n} bot",
  'grupos.contagem.many': "{n} bots",
  'grupos.contagem.other': "{n} bots",
}
