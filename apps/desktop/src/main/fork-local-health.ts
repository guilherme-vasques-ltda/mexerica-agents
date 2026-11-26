import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'

/** O que o `/health` do Hub local responde. A identidade (`computerId`, `binaryPath`, `binaryHash`
 * e `hostId`) só vem junto com a prova do token deste computador, e o `receipt` é o que prova que
 * quem respondeu tem esse token. */
export interface LocalHealth {
  environment?: string
  ok?: boolean
  hostId?: string
  computerId?: string
  binaryPath?: string
  binaryHash?: string
  busy?: boolean
  receipt?: string
}

/** Domínios da prova e do recibo, iguais aos do Hub (`host/src/fork/prova_health.rs`). */
const PROVA = 'mexerica/health-proof/v1'
const RECIBO = 'mexerica/health-receipt/v1'

function mac(token: string, rotulo: string, nonce: Buffer, extra?: string): string {
  const hmac = createHmac('sha256', token).update(rotulo).update(nonce)
  return (extra === undefined ? hmac : hmac.update(extra)).digest('hex')
}

/** A identidade que o Hub devolve, na ordem fixa que o recibo cobre. */
const identidade = (health: LocalHealth) =>
  [health.hostId ?? '', health.computerId ?? '', health.binaryPath ?? '', health.binaryHash ?? ''].join('\n')

/**
 * A prova do token local: um nonce novo e o HMAC-SHA256 do token sobre ele. O token em si nunca
 * viaja para a porta, então um processo que ocupe o lugar do Hub não recebe credencial nenhuma e
 * não tem como responder à prova.
 */
export function consultaProvada(token: string): { nonce: string; query: string } {
  const nonce = randomBytes(32)
  // base64url e hexadecimal já são seguros na URL: nada de escapar de novo.
  return {
    nonce: nonce.toString('base64url'),
    query: `?nonce=${nonce.toString('base64url')}&proof=${mac(token, PROVA, nonce)}`,
  }
}

/** O recibo confere: quem respondeu tem o mesmo token, para aquele nonce e aquela identidade. Um
 * listener substituto que copie os campos públicos não consegue produzi-lo. */
export function reciboConfere(token: string, nonce: string, health: LocalHealth): boolean {
  if (typeof health.receipt !== 'string' || !health.receipt) return false
  const esperado = Buffer.from(mac(token, RECIBO, Buffer.from(nonce, 'base64url'), identidade(health)), 'hex')
  const recebido = Buffer.from(health.receipt, 'hex')
  return recebido.length === esperado.length && timingSafeEqual(recebido, esperado)
}

/** O que a leitura com prova encontrou: o Hub com a identidade aceita, a prova recusada, ou nada
 * do outro lado (porta sem resposta). A diferença importa para quem decide entre repetir e parar. */
export type Proved = { kind: 'identified'; health: LocalHealth } | { kind: 'refused' } | { kind: 'unreachable' }

/** Só um objeto é saúde: `null`, texto ou número como corpo não carrega campo nenhum e a leitura
 * não pode quebrar por causa dele (um listener estranho não deve conseguir derrubar a vigia). */
function corpoSaude(body: unknown): LocalHealth | null {
  return body !== null && typeof body === 'object' ? (body as LocalHealth) : null
}

/** Prova o token e confere o recibo, dizendo qual dos três casos foi. */
export async function provarIdentidade(url: string, timeoutMs: number, token: string): Promise<Proved> {
  const prova = consultaProvada(token)
  let res: Response
  let health: LocalHealth | null
  try {
    res = await fetch(`${url}/health${prova.query}`, { signal: AbortSignal.timeout(timeoutMs) })
    if (!res.ok) return { kind: 'refused' }
    health = corpoSaude(await res.json())
  } catch {
    return { kind: 'unreachable' }
  }
  return health !== null && reciboConfere(token, prova.nonce, health) ? { kind: 'identified', health } : { kind: 'refused' }
}

/** Lê o `/health`: sem token, a resposta anônima (vida, ambiente e `busy`); com o token local, a
 * identidade do Hub, aceita só com o recibo certo. Sem prova válida, nada de identidade. */
export async function localHealth(url: string, timeoutMs: number, token?: string): Promise<LocalHealth | null> {
  if (token) {
    const proved = await provarIdentidade(url, timeoutMs, token)
    return proved.kind === 'identified' ? proved.health : null
  }
  try {
    const res = await fetch(`${url}/health`, { signal: AbortSignal.timeout(timeoutMs) })
    if (!res.ok) return null
    return corpoSaude(await res.json())
  } catch {
    return null
  }
}
