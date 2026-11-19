import type { DeviceBinding } from '../fork-account/service'
import { deviceIdentity, retireDeviceIdentity, discardDeviceIdentity } from '../fork-account/device-key'
import { signerFromKey, type DeviceSigner } from './crypto'
import { namespaceFor, type Deployment, type Principal } from './principal'

/** O identificador interno isola a custódia existente por conta e configuração. */
export async function deviceSigner(principal: Principal, mode: 'read' | 'sign' | 'register'): Promise<DeviceSigner> {
  if (!principal.current()) throw new Error('Account session changed.')
  const d = principal.deployment
  const scope = { origin: d.origin, issuer: d.issuer, deploymentId: d.deploymentId, manifestHash: d.manifestHash }
  if (mode === 'register') {
    // Never replace or silently migrate a legacy key; its reconciliation needs the
    // existing binding and retirement protocol, owned by the later factory seam.
    try {
      await deviceIdentity(principal.seal.userId, principal.current, 'read', scope)
      throw new Error('Legacy device identity requires reconciliation.')
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
    }
    if (!principal.current()) throw new Error('Account session changed.')
  }
  const identity = await deviceIdentity(principal.namespace, principal.current, mode, scope)
  if (!principal.current()) throw new Error('Account session changed.')
  const signer = signerFromKey(identity.key)
  return { publicKey: signer.publicKey, sign(message) {
    if (!principal.current()) throw new Error('Account session changed.')
    const result = signer.sign(message)
    if (!principal.current()) throw new Error('Account session changed.')
    return result
  } }
}

/** Só o vínculo comprovado da sessão capturada pode aposentar sua geração. */
export async function retireDevice(principal: Principal, binding: DeviceBinding): Promise<void> {
  const d = principal.deployment
  if (!principal.current() || binding.userId !== principal.seal.userId || binding.sessionId !== principal.seal.sessionId || binding.origin !== d.origin || binding.issuer !== d.issuer || binding.deploymentId !== d.deploymentId || binding.manifestHash !== d.manifestHash) throw new Error('Different device binding.')
  await retireDeviceIdentity({ ...binding, userId: principal.namespace }, principal.current)
  if (!principal.current()) throw new Error('Account session changed.')
}

/** Usa a guarda restrita da saída; não depende de uma sessão normal após o fence. */
export async function retireCapturedDevice(binding: DeviceBinding, deployment: Deployment, current: () => boolean): Promise<void> {
  if (!current() || binding.origin !== deployment.origin || binding.issuer !== deployment.issuer || binding.deploymentId !== deployment.deploymentId || binding.manifestHash !== deployment.manifestHash) throw new Error('Different device binding.')
  await retireDeviceIdentity({ ...binding, userId: namespaceFor(deployment, binding.userId) }, current)
  if (!current()) throw new Error('Account operation changed.')
}

/** D185: local cleanup of only the captured deployment/account namespace. */
export async function discardCapturedDevice(binding: DeviceBinding, deployment: Deployment, current: () => boolean): Promise<void> {
  if (!current() || binding.origin !== deployment.origin || binding.issuer !== deployment.issuer || binding.deploymentId !== deployment.deploymentId || binding.manifestHash !== deployment.manifestHash) throw new Error('Different device binding.')
  await discardDeviceIdentity(namespaceFor(deployment, binding.userId), current)
  if (!current()) throw new Error('Account operation changed.')
}
