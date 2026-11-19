import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/contexto/custo.ts'

// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-908 C8
  'contexto.custo.titulo': 'Estimated cost of this conversation',
  'contexto.custo.parcial': 'Partial: {n} of {total} turns without a price',
  'contexto.custo.vazio': 'No turns recorded in this conversation since the Hub update.',
  'contexto.custo.semTokens': 'No tokens recorded',
  'contexto.custo.falha': 'Cost unavailable right now.',
  'contexto.custo.nota': "Estimate of the API equivalent. On a subscription you don't pay per token; what counts is the account's limit.",
  'contexto.custo.parte.entrada': 'Input {n}',
  'contexto.custo.parte.cacheLido': 'Cache read {n}',
  'contexto.custo.parte.cacheGravado': 'Cache write {n}',
  'contexto.custo.parte.saida': 'Output {n}',
}
