import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/instrucoes.ts'

// Instruções do dono (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "instrucoes.pagina": "指令",
  "instrucoes.pagina.lead": "只需写一次你希望机器人如何工作，对所有机器人生效，无需逐个修改。",
  "instrucoes.geral.titulo": "适用于我的所有机器人",
  "instrucoes.geral.ajuda": "对这台电脑上任何空间里的每个机器人生效。",
  "instrucoes.geral.exemplo": "例如：回答简短，用通俗的语言。不要问我执行细节：选合理的默认做法，并用一行告诉我。",
  "instrucoes.space.titulo": "在此空间",
  "instrucoes.space.ajuda": "仅对当前打开空间里的机器人生效，并叠加在所有机器人的文字之上。",
  "instrucoes.space.exemplo": "例如：这里的工作面向客户：语气正式，不用俚语。",
  "instrucoes.contador": "{n}/{max}",
  "instrucoes.estado.salvo": "已保存。从每个机器人的下一条消息起生效，无需重启。",
  "instrucoes.estado.vazio": "为空。不会向机器人的指令添加任何内容。",
  "instrucoes.estado.naoSalvo": "未保存。",
  "instrucoes.estado.salvando": "正在保存…",
  "instrucoes.estado.lendo": "正在读取指令…",
  "instrucoes.estado.conflito": "文字在别处已更改。重试会重新读取，并把你的文字保存到新版本之上。",
  "instrucoes.acao.salvar": "保存",
  "instrucoes.acao.tentarDeNovo": "重试",
  "instrucoes.falha.envio": "现在无法保存，请重试。",
  "instrucoes.falha.leitura": "现在无法读取指令。",
  "instrucoes.falha.limite": "文字超出了字符限制。",
  "instrucoes.falha.invalido": "文字中有不可见字符或控制字符。请删除后重试。",
  "instrucoes.falha.spaceSumiu": "此空间已不存在。",
  "instrucoes.falha.soDono": "只有所有者在这台电脑上才能更改指令。",
  "instrucoes.falha.hubAntigo": "请更新 Hub 以使用指令。",
  "instrucoes.semConexao": "未连接到 Hub。连接恢复后会显示指令。",
  "instrucoes.ordem.titulo": "顺序：",
  "instrucoes.ordem.texto": "你写在机器人本身的内容优先于“在此空间”，“在此空间”又优先于“适用于我的所有机器人”。这些只是指引：不授予权限、工具、模型或账户。",
}
