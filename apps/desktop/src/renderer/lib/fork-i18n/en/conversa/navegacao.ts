import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/navegacao.ts'

// D432: conversa/navegacao (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // Barrinha, linha "Novas" e "Ir para o fim" (docs/reference/design/barrinha-novas-20261107).
  'conversa.navegacao.rotulo': "Conversation timeline",
  'conversa.navegacao.ir': "Go to {hora}: {texto}",
  'conversa.navegacao.irGrupo': "Go to {hora} · {n} messages: {texto}",
  'conversa.navegacao.semTexto': "message",
  'conversa.navegacao.novas': "New",
  'conversa.navegacao.novasRotulo': "New messages from here",
  'conversa.navegacao.fim': "Jump to the end",
  'conversa.navegacao.fimNovas.one': "{n} new",
  'conversa.navegacao.fimNovas.other': "{n} new",
}
