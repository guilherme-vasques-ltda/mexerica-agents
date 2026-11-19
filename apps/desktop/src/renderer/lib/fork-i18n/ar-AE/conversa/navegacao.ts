import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/navegacao.ts'

// D432: conversa/navegacao (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // Barrinha, linha "Novas" e "Ir para o fim" (docs/reference/design/barrinha-novas-20261107).
  'conversa.navegacao.rotulo': "الخط الزمني للمحادثة",
  'conversa.navegacao.ir': "الانتقال إلى {hora}: {texto}",
  'conversa.navegacao.irGrupo': "الانتقال إلى {hora} · {n} رسائل: {texto}",
  'conversa.navegacao.semTexto': "رسالة",
  'conversa.navegacao.novas': "جديد",
  'conversa.navegacao.novasRotulo': "رسائل جديدة من هنا",
  'conversa.navegacao.fim': "الانتقال إلى النهاية",
  'conversa.navegacao.fimNovas.zero': "لا جديد",
  'conversa.navegacao.fimNovas.one': "رسالة جديدة واحدة",
  'conversa.navegacao.fimNovas.two': "رسالتان جديدتان",
  'conversa.navegacao.fimNovas.few': "{n} رسائل جديدة",
  'conversa.navegacao.fimNovas.many': "{n} رسالة جديدة",
  'conversa.navegacao.fimNovas.other': "{n} رسالة جديدة",
}
