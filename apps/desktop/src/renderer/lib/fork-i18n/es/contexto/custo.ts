import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/contexto/custo.ts'

// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-908 C8
  'contexto.custo.titulo': 'Costo estimado de esta conversación',
  'contexto.custo.parcial': 'Parcial: {n} de {total} turnos sin precio',
  'contexto.custo.vazio': 'No hay turnos registrados en esta conversación desde la actualización del Hub.',
  'contexto.custo.semTokens': 'Sin tokens registrados',
  'contexto.custo.falha': 'Costo no disponible ahora.',
  'contexto.custo.nota': 'Estimación del equivalente en API. Con la suscripción no pagas por token; lo que cuenta es el límite de la cuenta.',
  'contexto.custo.parte.entrada': 'Entrada {n}',
  'contexto.custo.parte.cacheLido': 'Caché leída {n}',
  'contexto.custo.parte.cacheGravado': 'Caché escrita {n}',
  'contexto.custo.parte.saida': 'Salida {n}',
}
