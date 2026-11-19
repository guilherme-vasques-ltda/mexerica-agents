import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/memoria/sugestoes.ts'

// DON-903: memoria/sugestoes (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // D389: Sugestões na Memória do bot (views/fork/memoria-auto) e o contador do cartão.
  'memoria.sugestoes.esperando': "Suggestions waiting: {n}",
  'memoria.sugestoes.aba': "Suggestions",
  'memoria.sugestoes.lead': "What bots want to keep. None of it is in memory until you approve it.",
  'memoria.sugestoes.vazio': "No suggestions waiting.",
  'memoria.sugestoes.aprovar': "Approve",
  'memoria.sugestoes.descartar': "Discard",
  'memoria.sugestoes.acao.remember': "New memory",
  'memoria.sugestoes.acao.revise': "Change to a memory",
  'memoria.sugestoes.acao.archive': "Archive a memory",
  'memoria.sugestoes.acao.confirm': "Confirm: you said this",
  'memoria.sugestoes.acao.reclassify': "Confirm from your message",
  'memoria.sugestoes.acao.note': "Note {arquivo}",
  'memoria.sugestoes.removerNota': "Remove this note",
  'memoria.sugestoes.compartilhada': "For all your bots",
  'memoria.sugestoes.mudou': "Memory changed after this suggestion. Nothing was overwritten.",
  'memoria.sugestoes.semMudanca': "It was already in memory.",
  'memoria.sugestoes.erro': "Couldn't finish. Try again.",
}
