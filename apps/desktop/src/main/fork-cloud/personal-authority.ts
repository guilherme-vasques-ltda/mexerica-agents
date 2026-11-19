import { buildConfig } from '../environment'
import type { Principal } from './principal'
import { inspectPersonalManifest, type PersonalManifest } from './personal-protocol'
import type { PersonalBootstrapStatus } from '../../shared/fork-cloud'

/** Única admissão Main: constante do build. Não aceita raiz/manifesto como argumento. */
export function personalAuthority(p: Principal): PersonalManifest | null {
  if (!p.current() || buildConfig.environment !== p.deployment.env || !buildConfig.deploymentManifest) return null
  try {
    const parsed = inspectPersonalManifest(Buffer.from(JSON.stringify(buildConfig.deploymentManifest)))
    const c = parsed.context, d = p.deployment
    if (c[2] !== d.env || c[3] !== d.origin || c[4] !== d.issuer || c[5] !== d.deploymentId || c[6] !== d.manifestHash || !p.current()) return null
    return parsed
  } catch { return null }
}

/** Observação fechada C1: manifesto válido sozinho não torna vínculo disponível. */
export function personalBootstrapStatus(p: Principal): PersonalBootstrapStatus {
  return Object.freeze({ profile: 'personal-linux-v1', wire: 1, available: false, reason: personalAuthority(p) ? 'helperRequired' : 'updateRequired' })
}
