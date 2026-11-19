/** Exclusivo da demo. Nenhuma ponte desse módulo entra no preload/produto. */
import type { Intent, Operation, Place, PublicMaterial } from '../../shared/fork-workplaces.ts'
import { decodeManifest, methods } from '../../shared/fork-workplaces.ts'
import { workplaceSimulation } from './story.ts'

export class FakeWorkplaces {
  enabled = false
  scenario: 'ready' | 'api' | 'disconnected' | 'error' | 'slow' = 'ready'
  private places = new Map<string, Place>()
  private materials = new Map<string, PublicMaterial>()
  private operations = new Map<string, Operation>()
  private hub: () => string
  constructor(hub: () => string) { this.hub = hub }
  async call(method: string, body: Record<string, unknown>) {
    if (!this.enabled) throw Error('workplace_method_unavailable_in_demo')
    if (![...Object.values(methods), 'donnaWorkplaceAccess', 'donnaWorkplaceGuide', 'donnaWorkplaceList', 'donnaWorkplaceInspectStatus', 'donnaWorkplaceInspectCancel'].includes(method)) throw Error('workplace_method_unknown')
    const hubId = this.hub()
    const result = (value: object) => structuredClone({ ...value, hubId })
    if (['donnaWorkplaceAccess', 'donnaWorkplaceList', 'donnaWorkplaceGuide'].includes(method)
      && (!body || typeof body !== 'object' || Array.isArray(body) || Object.keys(body).length)) throw Error('workplace_request_failed')
    if (method === 'donnaWorkplaceGuide') {
      return result({ guide: { schema: 1, hubVersion: 'demo-04', text: 'DEMONSTRAÇÃO — roteiro sintético, sem comandos reais.\n\n1. Cadastre uma máquina fictícia.\n2. Gere somente material público fictício.\n3. O preparo e o login são pessoais no executor; esta demo não instala nem conecta nada.\n4. Confira identidade e conexão fictícias em passos separados.\n5. Consulte os estados simulados. Ajudantes permanecem indisponíveis.\n\nSenha, código, token, chave privada e perfil de login nunca entram aqui. O guia real acompanha a mesma versão do Hub Local.' } })
    }
    if (method === 'donnaWorkplaceAccess') return result({ available: true, origin: 'local', helpers: 'indisponivel' })
    if (method === 'donnaWorkplaceList') {
      const latest = new Map<string, Operation>()
      for (const op of this.operations.values()) {
        const place = this.places.get(op.placeId)
        if (place && place.hubId === hubId && op.hubId === hubId && op.authority === 'local'
          && op.admissionRevision === place.admissionRevision && op.connectionRevision === place.connectionRevision) latest.set(place.id, op)
      }
      return result({ places: [...this.places.values()].filter((place) => place.hubId === hubId), operations: [...latest.values()] })
    }
    if (method === 'donnaWorkplaceInspectStatus' || method === 'donnaWorkplaceInspectCancel') {
      const op = this.operations.get(String(body.operationId))
      const place = op && this.places.get(op.placeId)
      if (!op || op.hubId !== hubId || !place || place.hubId !== hubId || op.authority !== 'local'
        || op.admissionRevision !== place.admissionRevision || op.connectionRevision !== place.connectionRevision) throw Error('operation_invalidated')
      if (method === 'donnaWorkplaceInspectCancel' && op.phase === 'verificando') op.phase = 'cancelada_incerta'
      return result({ operation: op })
    }
    const intent = body.intent as Intent
    const target = body.target as Place | null
    if (!intent || methods[intent.type] !== method) throw Error('intent_mismatch')
    if (intent.type === 'create') {
      if (target !== null && target !== undefined) throw Error('target_not_allowed')
      if ([...this.places.values()].some((p) => p.hubId === hubId && p.resourceId === intent.resource_id)) throw Error('resource_invalid')
      const place: Place = { id: crypto.randomUUID(), hubId, name: `${intent.name} · ${workplaceSimulation}`, resourceId: intent.resource_id,
        kind: 'cloud', provider: 'boat', revision: 1, admissionRevision: 1, connectionRevision: 0, capacityRevision: 0,
        prepared: false, pinned: false, connected: false, helpers: 'indisponivel' }
      this.places.set(place.id, place)
      return result({ place, removed: false })
    }
    const place = target && this.places.get(target.id)
    if (!place || place.hubId !== hubId) throw Error('target_invalidated')
    if (!place || target?.revision !== place.revision || target.admissionRevision !== place.admissionRevision || target.connectionRevision !== place.connectionRevision) throw Error('revision_conflict')
    const material = this.materials.get(place.id)
    if (material && (material.hubId !== hubId || material.placeId !== place.id || material.resourceId !== place.resourceId)) throw Error('target_invalidated')
    const manifest = intent.type === 'pin_runner' ? decodeManifest(intent.manifest) : null
    if (intent.type === 'pin_runner' || intent.type === 'refix' || intent.type === 'remove' || (intent.type === 'prepare_connection' && intent.rotate)) {
      for (const op of this.operations.values()) if (op.hubId === hubId && op.placeId === place.id && op.phase === 'verificando') op.phase = 'cancelada_incerta'
    }
    switch (intent.type) {
      case 'rename': place.name = `${intent.name} · ${workplaceSimulation}`; place.revision++; break
      case 'prepare_connection': {
        if (!place.prepared || intent.rotate) place.admissionRevision++
        place.prepared = true
        const publicMaterial: PublicMaterial = material ?? { sshPublic: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMqTqxTn1ZtscNfOB0sVubVwJfvlbhGGuxRVp1ONvhZw donna-ponte-simulada',
          controlPublic: 'A'.repeat(43), hubId, placeId: place.id, resourceId: place.resourceId, runnerPort: 47631, runnerPin: null, manifest: null }
        this.materials.set(place.id, publicMaterial)
        return result({ place, publicMaterial })
      }
      case 'pin_runner': {
        place.pinned = true; place.admissionRevision++
        if (material) { material.runnerPin = intent.pin; material.manifest = manifest }
        break
      }
      case 'refix': place.connected = true; place.connectionRevision++; break
      case 'remove': this.places.delete(place.id); this.materials.delete(place.id); return result({ place, removed: true })
      case 'inspect_start': {
        if (!place.prepared || !place.pinned || !place.connected) throw Error('prepare_required')
        if ([...this.operations.values()].some((op) => op.hubId === hubId && op.placeId === place.id && op.phase === 'verificando')) throw Error('inspection_conflict')
        const op: Operation = { id: crypto.randomUUID(), placeId: place.id, hubId, admissionRevision: place.admissionRevision, connectionRevision: place.connectionRevision,
          authority: 'local', intent: String(body.clientRequestId), deadline: Date.now() + 60_000, observedAt: null, phase: 'verificando', reason: null,
          observation: { connection: 'nao_verificado', identity: 'nao_verificado', preparation: 'nao_verificado', account: 'nao_verificado', helpers: 'indisponivel' } }
        this.operations.set(op.id, op)
        const scenario = this.scenario
        setTimeout(() => {
          const current = this.places.get(op.placeId)
          if (op.phase !== 'verificando' || !current || current.hubId !== hubId || op.hubId !== hubId
            || current.admissionRevision !== op.admissionRevision || current.connectionRevision !== op.connectionRevision) return
          op.phase = scenario === 'error' ? 'falhou' : 'concluida'
          op.observedAt = Date.now()
          if (op.phase === 'concluida') op.observation = { connection: 'conferido', identity: 'conferido', preparation: 'conferido',
            account: scenario === 'api' ? 'api_nao_admitida' : scenario === 'disconnected' ? 'desconectado' : 'assinatura_oficial', helpers: 'indisponivel' }
        }, scenario === 'slow' ? 45_000 : 2200)
        return result({ place, operation: op })
      }
    }
    return result({ place, removed: false })
  }
}
