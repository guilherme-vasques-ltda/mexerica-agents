/** UMA fonte fechada admitida. Vazia: nenhuma CA/destino real habilitado no PR3. */
export interface SshTrust {
  readonly schema: 1; readonly qualificationId: string; readonly evidenceDigest: string
  readonly ca: string; readonly caFingerprint: string; readonly host: string; readonly port: number; readonly user: string
  readonly principal: string; readonly admittedUntil: number
}
const ADMITTED: readonly Readonly<SshTrust>[] = Object.freeze([])
export function personalSshTrust(id: string): Readonly<SshTrust> | null { return ADMITTED.find(r => r.qualificationId === id) ?? null }
