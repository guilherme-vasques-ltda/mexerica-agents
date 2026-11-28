// O login de conector na Open só abre a página que o próprio Hub devolveu em connectorSignIn para o ouvinte deste
// app: o processo principal guarda esse endereço quando a resposta passa por ele e o entrega uma única vez.
let plano: string | null = null

export function guardarPlano(resposta: unknown) {
  const r = typeof resposta === 'object' && resposta !== null ? resposta as { url?: unknown; callback?: unknown } : {}
  plano = r.callback === 'desktop' && typeof r.url === 'string' ? r.url : null
}

export function consumirPlano(url: string): boolean {
  const igual = plano !== null && plano === url
  plano = null
  return igual
}
