import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/grupos.ts'

// Grupos de bots (registro.ts). Chaves completas; docs/guides/idiomas.md.
// Escrito a partir do significado do pt-BR novo (grupos.ts), sem consultar texto anterior.
export const textos: Traducao<typeof fonte> = {
  'grupos.menu.novo': "إنشاء مجموعة",
  'grupos.nomePadrao': "مجموعة جديدة",
  'grupos.nome.rotulo': "اسم المجموعة",
  'grupos.menu.renomear': "إعادة تسمية المجموعة",
  'grupos.menu.apagar': "حذف المجموعة",
  'grupos.menu.recolher': "طيّ المجموعة",
  'grupos.menu.mostrar': "إظهار المجموعة",
  'grupos.bot.mover': "وضع في {grupo}",
  'grupos.bot.tirar': "إخراج من المجموعة",
  'grupos.bot.novoGrupo': "مجموعة جديدة تضم {bot}",
  'grupos.foraDeGrupo': "(خارج المجموعات)",
  'grupos.vazio': "لا توجد بوتات في هذه المجموعة.",
  'grupos.dica': "اسحب بوتًا إلى مجموعة أو استخدم قائمته.",
  'grupos.filtro.todos': "كل المجموعات",
  'grupos.erro.nomeInvalido': "لا يمكن أن يكون الاسم فارغًا أو أطول من 40 حرفًا أو يحتوي على أحرف غير مرئية أو أحرف تحكّم.",
  'grupos.erro.nomeUsado': "توجد مجموعة أخرى بهذا الاسم في هذه المساحة.",
  'grupos.erro.grupoSumiu': "هذه المجموعة لم تعد موجودة.",
  'grupos.erro.botSumiu': "هذا البوت لم يعد موجودًا.",
  'grupos.erro.atualizando': "يجري تحديث المجموعات. حاول مرة أخرى بعد قليل.",
  'grupos.erro.generico': "تعذّر تحديث المجموعات. حاول مرة أخرى.",
  'grupos.contagem.zero': "لا بوتات",
  'grupos.contagem.one': "بوت واحد",
  'grupos.contagem.two': "بوتان",
  'grupos.contagem.few': "{n} بوتات",
  'grupos.contagem.many': "{n} بوتًا",
  'grupos.contagem.other': "{n} بوت",
}
