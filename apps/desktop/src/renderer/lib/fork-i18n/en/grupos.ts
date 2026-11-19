import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/grupos.ts'

// Grupos de bots (registro.ts). Chaves completas; docs/guides/idiomas.md.
// Escrito a partir do significado do pt-BR novo (grupos.ts), sem consultar texto anterior.
export const textos: Traducao<typeof fonte> = {
  'grupos.menu.novo': "Create group",
  'grupos.nomePadrao': "New group",
  'grupos.nome.rotulo': "Group name",
  'grupos.menu.renomear': "Rename group",
  'grupos.menu.apagar': "Delete group",
  'grupos.menu.recolher': "Collapse group",
  'grupos.menu.mostrar': "Show group",
  'grupos.bot.mover': "Put in {grupo}",
  'grupos.bot.tirar': "Remove from group",
  'grupos.bot.novoGrupo': "Start a group with {bot}",
  'grupos.foraDeGrupo': "(ungrouped)",
  'grupos.vazio': "No bots in this group.",
  'grupos.dica': "Drag a bot onto a group, or use its menu.",
  'grupos.filtro.todos': "All groups",
  'grupos.erro.nomeInvalido': "The name can't be empty, run past 40 characters, or contain invisible or control characters.",
  'grupos.erro.nomeUsado': "Another group in this Space already has that name.",
  'grupos.erro.grupoSumiu': "This group no longer exists.",
  'grupos.erro.botSumiu': "This bot no longer exists.",
  'grupos.erro.atualizando': "Groups are being updated. Try again in a moment.",
  'grupos.erro.generico': "Couldn't update the groups. Try again.",
  'grupos.contagem.one': "{n} bot",
  'grupos.contagem.other': "{n} bots",
}
