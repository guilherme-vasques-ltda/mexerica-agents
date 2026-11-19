import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/navegacao.ts'

// D432: conversa/navegacao (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // Barrinha, linha "Novas" e "Ir para o fim" (docs/reference/design/barrinha-novas-20261107).
  'conversa.navegacao.rotulo': "Fil de la conversation",
  'conversa.navegacao.ir': "Aller à {hora} : {texto}",
  'conversa.navegacao.irGrupo': "Aller à {hora} · {n} messages : {texto}",
  'conversa.navegacao.semTexto': "message",
  'conversa.navegacao.novas': "Nouveaux",
  'conversa.navegacao.novasRotulo': "Nouveaux messages à partir d’ici",
  'conversa.navegacao.fim': "Aller à la fin",
  'conversa.navegacao.fimNovas.one': "{n} nouveau",
  'conversa.navegacao.fimNovas.many': "{n} nouveaux",
  'conversa.navegacao.fimNovas.other': "{n} nouveaux",
}
