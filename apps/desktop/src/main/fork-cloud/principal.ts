import { createHash } from 'node:crypto'
import type { AccountService, CloudSeal } from '../fork-account/service'
import { accountConfig } from '../fork-account/config'
import { buildConfig } from '../environment'
import { bootstrapAuthority } from './bootstrap-authority'
import { personalBootstrapStatus } from './personal-authority'
import type { PersonalBootstrapStatus } from '../../shared/fork-cloud'

export type Deployment = { readonly deploymentId: string; readonly env: 'main' | 'dev'; readonly issuer: string; readonly origin: string; readonly manifestHash: string }
export interface Principal {
  readonly seal: CloudSeal
  readonly deployment: Deployment
  readonly namespace: string
  /** Loading hint only; BootstrapService revalidates the fixed artifact before every action. */
  readonly bootstrapAvailable?: boolean
  readonly personalBootstrapStatus?: PersonalBootstrapStatus
  current(): boolean
  token(): Promise<string>
}

export function namespaceFor(deployment: Deployment, userId: string): string {
  return createHash('sha256').update(JSON.stringify([deployment.deploymentId, deployment.env, deployment.issuer, deployment.origin, deployment.manifestHash, userId])).digest('hex')
}

/** Captura uma vez a sessão real; nenhuma resposta de health escolhe a configuração. */
export function capturePrincipal(account: AccountService): Principal | null {
  const config = accountConfig(buildConfig)
  const seal = account.captureCloudSession()
  if (config.kind !== 'configured' || !seal || !account.isCloudSessionCurrent(seal)) return null
  const scope = account.deviceIdentityScope()
  if (!scope || scope.origin !== config.cloudOrigin || scope.issuer !== config.issuer || scope.deploymentId !== config.deploymentId || scope.manifestHash !== config.manifestHash) return null
  const deployment = Object.freeze({ deploymentId: config.deploymentId, env: config.env, issuer: config.issuer, origin: config.cloudOrigin, manifestHash: config.manifestHash })
  const namespace = namespaceFor(deployment, seal.userId)
  const current = () => account.isCloudSessionCurrent(seal)
  const principal = Object.freeze({ seal, deployment, namespace, current, async token() {
    if (!current()) throw new Error('Account session changed.')
    const token = await account.tokenForCloud(seal)
    if (!current()) throw new Error('Account session changed.')
    return token
  } })
  return Object.freeze({ ...principal, bootstrapAvailable: bootstrapAuthority(principal) !== null, personalBootstrapStatus: personalBootstrapStatus(principal) })
}
