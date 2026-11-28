// O login de conector na Open só abre a página que o próprio Hub devolveu em connectorSignIn para o ouvinte deste
// app: o processo principal guarda esse endereço, preso à tentativa que o pediu, e o entrega uma única vez, só ao
// ouvinte vivo daquela mesma tentativa. Só existe um plano por vez; uma tentativa nova, o cancelamento, a conclusão e o
// fim do ouvinte o descartam, e uma resposta que chega depois de a tentativa acabar não repõe nada.
let plano: { tentativa: string; url: string } | null = null
let ouvinteVivo: () => string | null = () => null

/** Liga o plano ao ouvinte: devolve o identificador da tentativa do ouvinte que está ligado agora. */
export function ligarOuvinte(vivo: () => string | null) {
  ouvinteVivo = vivo
}

function tentativaDe(corpo: unknown): string | null {
  const t = typeof corpo === 'object' && corpo !== null ? (corpo as { tentativa?: unknown }).tentativa : null
  return typeof t === 'string' ? t : null
}

export function guardarPlano(resposta: unknown, pedido: unknown) {
  const r = typeof resposta === 'object' && resposta !== null ? resposta as { url?: unknown; callback?: unknown } : {}
  const tentativa = tentativaDe(pedido)
  const vale = tentativa !== null && ouvinteVivo() === tentativa && r.callback === 'desktop' && typeof r.url === 'string'
  plano = vale ? { tentativa, url: r.url as string } : null
}

/** Sem `tentativa` no pedido, descarta qualquer plano; com ela, só o plano daquela tentativa. */
export function descartarPlano(pedido: unknown) {
  const tentativa = tentativaDe(pedido)
  if (tentativa === null || plano?.tentativa === tentativa) plano = null
}

/** O plano só vale para o ouvinte vivo da tentativa que o pediu: `nonce` é o identificador desse ouvinte. */
export function consumirPlano(url: string, nonce: string | null): boolean {
  const igual = plano !== null && nonce !== null && plano.tentativa === nonce && plano.url === url
  plano = null
  return igual
}
