import type { ForkCloudIntent, ForkCloudResult } from '../../shared/fork-cloud'

/** Main-process policy for a platform-designated hosted computer. Restored unknown
 * phases only read; the backend never signs the device's proof. The initial
 * offer window gates new work, never recovery or an already committed binding. */
export function managedAction(result: ForkCloudResult, computerId: string, canStart: boolean): ForkCloudIntent | null {
  if (result.recovery === 'registration') return { kind:'reconcileRegistration' }
  if (result.recovery) return null
  if (result.registration === 'unregistered') return canStart ? { kind:'registerDevice', name:'Donnabot' } : null
  if (result.registration !== 'registered') return null
  const b=result.bootstrap
  if (!b) return null
  if (b.computerId && b.computerId!==computerId) return null
  if (b.phase==='available' || b.phase==='prepared') return canStart ? {kind:'startBootstrap'} : null
  if (b.phase==='complete') return {kind:'connect',computerId}
  if (b.canProve && b.phase==='requested') return {kind:'proveBootstrap',bootstrapId:b.bootstrapId}
  if (['receipt','published','pinUnknown'].includes(b.phase) && b.canReconcile) return {kind:'reconcileBootstrap',bootstrapId:b.bootstrapId}
  if (b.canCheck && !['closed','complete'].includes(b.phase)) return {kind:'checkBootstrap',bootstrapId:b.bootstrapId}
  return null
}
