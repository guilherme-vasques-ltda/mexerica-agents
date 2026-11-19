import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/grupos.ts'

// Grupos de bots (registro.ts). Chaves completas; docs/guides/idiomas.md.
// Escrito a partir do significado do pt-BR novo (grupos.ts), sem consultar texto anterior.
export const textos: Traducao<typeof fonte> = {
  'grupos.menu.novo': "Crear grupo",
  'grupos.nomePadrao': "Nuevo grupo",
  'grupos.nome.rotulo': "Nombre del grupo",
  'grupos.menu.renomear': "Renombrar grupo",
  'grupos.menu.apagar': "Borrar grupo",
  'grupos.menu.recolher': "Contraer grupo",
  'grupos.menu.mostrar': "Mostrar grupo",
  'grupos.bot.mover': "Poner en {grupo}",
  'grupos.bot.tirar': "Sacar del grupo",
  'grupos.bot.novoGrupo': "Comenzar grupo con {bot}",
  'grupos.foraDeGrupo': "(fuera de grupo)",
  'grupos.vazio': "No hay bots en este grupo.",
  'grupos.dica': "Arrastra un bot a un grupo o usa su menú.",
  'grupos.filtro.todos': "Todos los grupos",
  'grupos.erro.nomeInvalido': "El nombre no puede quedar vacío, superar los 40 caracteres ni incluir caracteres invisibles o de control.",
  'grupos.erro.nomeUsado': "Ya hay otro grupo con ese nombre en este Space.",
  'grupos.erro.grupoSumiu': "Este grupo ya no existe.",
  'grupos.erro.botSumiu': "Este bot ya no existe.",
  'grupos.erro.atualizando': "Los grupos se están actualizando. Inténtalo de nuevo en unos instantes.",
  'grupos.erro.generico': "No se pudieron actualizar los grupos. Inténtalo de nuevo.",
  'grupos.contagem.one': "{n} bot",
  'grupos.contagem.many': "{n} bots",
  'grupos.contagem.other': "{n} bots",
}
