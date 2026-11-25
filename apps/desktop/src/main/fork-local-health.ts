import { createHmac, randomBytes } from 'node:crypto'

/** O que o `/health` do Hub local responde. A identidade (`computerId`, `binaryPath`, `binaryHash`
 * e `hostId`) só vem junto com a prova do token deste computador; sem ela a resposta é anônima. */
export interface LocalHealth {
  environment?: string
  ok?: boolean
  computerId?: string
  binaryPath?: string
  binaryHash?: string
  busy?: boolean
}

/** Domínio da prova, igual ao do Hub (`host/src/fork/prova_health.rs`). */
const ROTULO = 'mexerica/health-proof/v1'

/**
 * A prova do token local: um nonce novo e o HMAC-SHA256 do token sobre ele. O token em si nunca
 * viaja para a porta, então um processo que ocupe o lugar do Hub não recebe credencial nenhuma e
 * não tem como responder à prova; o Hub confere e só então devolve a identidade.
 */
export function provedQuery(token: string): string {
  const nonce = randomBytes(32)
  const proof = createHmac('sha256', token).update(ROTULO).update(nonce).digest('hex')
  // base64url e hexadecimal já são seguros na URL: nada de escapar de novo.
  return `?nonce=${nonce.toString('base64url')}&proof=${proof}`
}

/** Lê o `/health`: sem token, a resposta anônima (vida, ambiente e `busy`); com o token local, a
 * identidade do Hub. Um token que o Hub recusa não deixa nada para ler, e a rede fica em `null`. */
export async function localHealth(url: string, timeoutMs: number, token?: string): Promise<LocalHealth | null> {
  try {
    const res = await fetch(`${url}/health${token ? provedQuery(token) : ''}`, { signal: AbortSignal.timeout(timeoutMs) })
    if (!res.ok) return null
    return (await res.json()) as LocalHealth
  } catch {
    return null
  }
}
