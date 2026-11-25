/** O que o `/health` do Hub local responde. A identidade (`computerId`, `binaryPath`, `binaryHash`
 * e `hostId`) só vem junto com o token deste computador; sem ele a resposta é anônima. */
export interface LocalHealth {
  environment?: string
  ok?: boolean
  computerId?: string
  binaryPath?: string
  binaryHash?: string
  busy?: boolean
}

/** Lê o `/health`: sem token, a resposta anônima (vida, ambiente e `busy`); com o token local, a
 * identidade do Hub. Um token que o Hub recusa não deixa nada para ler, e a rede fica em `null`. */
export async function localHealth(url: string, timeoutMs: number, token?: string): Promise<LocalHealth | null> {
  try {
    const res = await fetch(`${url}/health`, {
      headers: token ? { authorization: `Bearer ${token}` } : undefined,
      signal: AbortSignal.timeout(timeoutMs),
    })
    if (!res.ok) return null
    return (await res.json()) as LocalHealth
  } catch {
    return null
  }
}
