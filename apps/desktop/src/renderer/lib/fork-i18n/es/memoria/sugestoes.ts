import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/memoria/sugestoes.ts'

// DON-903: memoria/sugestoes (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // D389: Sugestões na Memória do bot (views/fork/memoria-auto) e o contador do cartão.
  'memoria.sugestoes.esperando': "Sugerencias en espera: {n}",
  'memoria.sugestoes.aba': "Sugerencias",
  'memoria.sugestoes.lead': "Lo que los bots quieren guardar. Nada de esto está en la memoria hasta que lo apruebes.",
  'memoria.sugestoes.vazio': "No hay sugerencias en espera.",
  'memoria.sugestoes.aprovar': "Aprobar",
  'memoria.sugestoes.descartar': "Descartar",
  'memoria.sugestoes.acao.remember': "Nuevo recuerdo",
  'memoria.sugestoes.acao.revise': "Corrección de un recuerdo",
  'memoria.sugestoes.acao.archive': "Archivar un recuerdo",
  'memoria.sugestoes.acao.confirm': "Confirmar: tú lo dijiste",
  'memoria.sugestoes.acao.reclassify': "Confirmar por tu mensaje",
  'memoria.sugestoes.acao.note': "Nota {arquivo}",
  'memoria.sugestoes.removerNota': "Quitar esta nota",
  'memoria.sugestoes.compartilhada': "Para todos tus bots",
  'memoria.sugestoes.mudou': "La memoria cambió después de esta sugerencia. No se sobrescribió nada.",
  'memoria.sugestoes.semMudanca': "Ya estaba en la memoria.",
  'memoria.sugestoes.erro': "No se pudo completar. Inténtalo de nuevo.",
}
