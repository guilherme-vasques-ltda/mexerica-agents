// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos = {
  // DON-908 C8: custo estimado no balão de contexto (contexto.custo.*) e em Ajustes → Uso (conversa.custo.*).
  'contexto.custo.titulo': 'Custo estimado desta conversa',
  'contexto.custo.parcial': 'Parcial: {n} de {total} turnos sem preço',
  'contexto.custo.vazio': 'Nenhum turno registrado nesta conversa desde a atualização do Hub.',
  'contexto.custo.semTokens': 'Sem tokens registrados',
  'contexto.custo.falha': 'Custo indisponível agora.',
  'contexto.custo.nota': 'Estimativa do equivalente em API. Na assinatura você não paga por token; o que conta é o limite da conta.',
  'contexto.custo.parte.entrada': 'Entrada {n}',
  'contexto.custo.parte.cacheLido': 'Cache lido {n}',
  'contexto.custo.parte.cacheGravado': 'Cache gravado {n}',
  'contexto.custo.parte.saida': 'Saída {n}',
} as const
