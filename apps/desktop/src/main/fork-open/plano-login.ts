// O login de conector na Open só abre a página que o próprio Hub devolveu em connectorSignIn para o ouvinte deste
// app: o processo principal guarda esse endereço, preso à tentativa que o pediu, e o entrega uma única vez.
// Só existe um plano por vez: uma tentativa nova, o cancelamento ou a conclusão da tentativa o descartam.
let plano: { tentativa: string; url: string } | null = null

function tentativaDe(corpo: unknown): string | null {
  const t = typeof corpo === 'object' && corpo !== null ? (corpo as { tentativa?: unknown }).tentativa : null
  return typeof t === 'string' ? t : null
}

export function guardarPlano(resposta: unknown, pedido: unknown) {
  const r = typeof resposta === 'object' && resposta !== null ? resposta as { url?: unknown; callback?: unknown } : {}
  const tentativa = tentativaDe(pedido)
  plano = tentativa !== null && r.callback === 'desktop' && typeof r.url === 'string' ? { tentativa, url: r.url } : null
}

/** Sem `tentativa` no pedido, descarta qualquer plano; com ela, só o plano daquela tentativa. */
export function descartarPlano(pedido: unknown) {
  const tentativa = tentativaDe(pedido)
  if (tentativa === null || plano?.tentativa === tentativa) plano = null
}

export function consumirPlano(url: string): boolean {
  const igual = plano !== null && plano.url === url
  plano = null
  return igual
}
