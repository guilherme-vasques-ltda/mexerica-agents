import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/idioma.ts'

// DON-903: idioma (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'idioma.nome': '简体中文',
  'idioma.passo.titulo1': '你的所有机器人，',
  'idioma.passo.titulo2': '尽在一处。',
  'idioma.passo.subtitulo': 'Berga 在 Mexerica 中管理你的机器人团队，并在有事需要你关注时提醒你。',
  'idioma.passo.escolha': '选择语言',
  'idioma.passo.sugerido': '根据 Mac 的语言推荐',
  'idioma.passo.continuar': '继续',
  'idioma.passo.rodape': '之后可以在“设置”→“常规”中更改。',
  'idioma.passo.etapa': '第 {atual} 步，共 {total} 步',
  'idioma.aviso.migracao': '翻译工作仍在进行：账户登录和应用的大部分内容目前仍为葡萄牙语。',
  'idioma.aviso.bots': '机器人仍会用葡萄牙语回复。',
  'idioma.ajuste.secao': '语言',
  'idioma.ajuste.rotulo': '应用语言',
}
