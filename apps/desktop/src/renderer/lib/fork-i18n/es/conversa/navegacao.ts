import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/navegacao.ts'

// D432: conversa/navegacao (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // Barrinha, linha "Novas" e "Ir para o fim" (docs/reference/design/barrinha-novas-20261107).
  'conversa.navegacao.rotulo': "Línea de tiempo de la conversación",
  'conversa.navegacao.ir': "Ir a {hora}: {texto}",
  'conversa.navegacao.irGrupo': "Ir a {hora} · {n} mensajes: {texto}",
  'conversa.navegacao.semTexto': "mensaje",
  'conversa.navegacao.novas': "Nuevos",
  'conversa.navegacao.novasRotulo': "Mensajes nuevos desde aquí",
  'conversa.navegacao.fim': "Ir al final",
  'conversa.navegacao.fimNovas.one': "{n} nuevo",
  'conversa.navegacao.fimNovas.many': "{n} nuevos",
  'conversa.navegacao.fimNovas.other': "{n} nuevos",
}
