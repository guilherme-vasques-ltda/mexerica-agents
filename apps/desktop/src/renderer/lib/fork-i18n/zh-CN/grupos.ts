import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/grupos.ts'

// Grupos de bots (registro.ts). Chaves completas; docs/guides/idiomas.md.
// Escrito a partir do significado do pt-BR novo (grupos.ts), sem consultar texto anterior.
export const textos: Traducao<typeof fonte> = {
  'grupos.menu.novo': "创建分组",
  'grupos.nomePadrao': "新分组",
  'grupos.nome.rotulo': "分组名称",
  'grupos.menu.renomear': "重命名分组",
  'grupos.menu.apagar': "删除分组",
  'grupos.menu.recolher': "收起分组",
  'grupos.menu.mostrar': "显示分组",
  'grupos.bot.mover': "放入分组 {grupo}",
  'grupos.bot.tirar': "移出分组",
  'grupos.bot.novoGrupo': "新建分组并放入 {bot}",
  'grupos.foraDeGrupo': "（未分组）",
  'grupos.vazio': "此分组里还没有机器人。",
  'grupos.dica': "把机器人拖到分组上，或使用它的菜单。",
  'grupos.filtro.todos': "全部分组",
  'grupos.erro.nomeInvalido': "名称不能为空，不能超过 40 个字符，也不能包含不可见字符或控制字符。",
  'grupos.erro.nomeUsado': "此空间中已有同名的分组。",
  'grupos.erro.grupoSumiu': "此分组已不存在。",
  'grupos.erro.botSumiu': "此机器人已不存在。",
  'grupos.erro.atualizando': "分组正在更新，请稍后再试。",
  'grupos.erro.generico': "无法更新分组，请重试。",
  'grupos.contagem.other': "{n} 个机器人",
}
