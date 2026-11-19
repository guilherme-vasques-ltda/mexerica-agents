import { decodeGuideReply, decodeReply, methods, type Intent, type Operation, type Place, type PublicMaterial, type Reply, type WorkplaceGuide } from '../../shared/fork-workplaces.ts'

interface Link { call<T>(method: string, body: unknown, timeout?: number): Promise<T> }
export interface Source {
  readonly connectedClient: Link | null; readonly hello: { hostId: string } | null; readonly principalEpoch: number
  subscribe(listener: () => void): () => void
}
interface Origin { link: Link; hub: string; epoch: number; generation: number }
/** Memória privada de uma página/conexão. Sem mirror, localStorage ou fila offline. */
export class Workplaces {
  places: Place[] = []
  material: PublicMaterial | null = null
  operation: Operation | null = null
  operations: Operation[] = []
  available = false
  busy = false
  /** DON-917 (D507): código da falha; a tela escolhe a frase (`lugares.erro.*`). */
  error: WorkplaceError | null = null
  generation = 0
  private version = 0
  private projectionEpoch = 0
  private listeners = new Set<() => void>()
  private origin: Origin | null = null
  private stop: () => void
  private disposed = false
  private source: Source
  constructor(source: Source) {
    this.source = source
    this.stop = source.subscribe(() => this.reconcile())
    this.reconcile()
  }
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getVersion = () => this.version
  private changed() { this.version++; for (const listener of this.listeners) listener() }
  private reconcile() {
    if (this.disposed) return
    const { connectedClient: link, principalEpoch: epoch } = this.source
    const hub = this.source.hello?.hostId
    if ((!link || !hub) && this.origin === null) return
    if (this.origin?.link === link && this.origin.hub === hub && this.origin.epoch === epoch) return
    this.clear()
    this.origin = link && hub ? { link, hub, epoch, generation: this.generation } : null
    this.changed()
  }
  private clear() {
    this.generation++; this.places = []; this.material = null; this.operation = null; this.operations = []
    this.available = false; this.busy = false; this.error = null
  }
  dispose() { this.disposed = true; this.stop(); this.origin = null; this.clear(); this.changed(); this.listeners.clear() }
  private current(o: Origin) {
    return !this.disposed && this.origin === o && this.generation === o.generation && this.source.connectedClient === o.link && this.source.hello?.hostId === o.hub && this.source.principalEpoch === o.epoch
  }
  async load() {
    this.reconcile()
    if (!this.origin || this.busy) return
    const o = this.origin
    const projection = this.projectionEpoch
    this.busy = true; this.error = null; this.changed()
    try {
      const access = decodeReply(await o.link.call('donnaWorkplaceAccess', {}), o.hub)
      if (!this.current(o)) return
      if (!access.available) throw Error('origin_not_admitted')
      const result = decodeReply(await o.link.call('donnaWorkplaceList', {}), o.hub)
      if (!this.current(o)) return
      if (projection !== this.projectionEpoch) return
      if (!result.places || !result.operations) throw Error('workplace_response_invalid')
      if (this.material) {
        const old = this.places.find((p) => p.id === this.material!.placeId)
        const fresh = result.places.find((p) => p.id === this.material!.placeId)
        if (!old || !fresh || old.admissionRevision !== fresh.admissionRevision) this.material = null
      }
      this.available = true; this.places = result.places; this.operations = result.operations ?? []
      if (this.material && !this.places.some((p) => p.id === this.material!.placeId && p.prepared)) this.material = null
    } catch (error) { if (this.current(o)) this.refuse(error, true) }
    finally { if (this.current(o)) { this.busy = false; this.changed() } }
  }
  /** `ao_carregar`: só método desconhecido do Hub (404 `unknown method`) é Hub sem o recurso; timeout e falha interna seguem genéricos. */
  private refuse(error: unknown, aoCarregar = false) {
    this.places = []; this.material = null; this.operation = null; this.operations = []; this.available = false
    const code = errorMessage(error)
    this.error = aoCarregar && code === 'generico' && metodoDesconhecido(error) ? 'semRecurso' : code
  }
  async mutate(intent: Intent, place: Place | null, requestId = crypto.randomUUID()): Promise<Reply | null> {
    return this.call(methods[intent.type], { clientRequestId: requestId, target: place ? {
      id: place.id, revision: place.revision, admissionRevision: place.admissionRevision, connectionRevision: place.connectionRevision,
    } : null, intent })
  }
  async status(operation: Operation) { return this.call('donnaWorkplaceInspectStatus', { operationId: operation.id }, false) }
  async cancel(operation: Operation) { return this.call('donnaWorkplaceInspectCancel', { operationId: operation.id, clientRequestId: crypto.randomUUID() }) }
  operationFor(placeId: string) { return this.operations.find((op) => op.placeId === placeId) ?? null }
  /** Leitura própria: não altera erro, material, progresso ou fila das intenções. */
  async readGuide(): Promise<WorkplaceGuide | null> {
    this.reconcile()
    const origin = this.origin
    if (!origin || !this.available) return null
    try {
      const value = await origin.link.call('donnaWorkplaceGuide', {}, 20_000)
      if (!this.current(origin)) return null
      return decodeGuideReply(value, origin.hub)
    } catch (error) {
      if (!this.current(origin)) return null
      throw error
    }
  }
  private async call(method: string, body: unknown, busy = true): Promise<Reply | null> {
    this.reconcile()
    const o = this.origin
    if (!o || !this.available || (busy && this.busy)) return null
    if (busy) this.busy = true
    if (busy) this.projectionEpoch++
    const polling = method === 'donnaWorkplaceInspectStatus'
    const pollId = polling ? (body as { operationId: string }).operationId : null
    this.error = null
    // Nova intenção descarta a observação anterior antes de enviar.
    if (!polling && method !== 'donnaWorkplaceInspectCancel') {
      this.material = null
      const intent = (body as { intent?: Intent }).intent
      const invalidates = intent?.type !== 'rename' && !(intent?.type === 'prepare_connection' && !intent.rotate)
      const target = (body as { target?: { id: string } }).target
      if (invalidates) {
        this.operation = null
        if (target) this.operations = this.operations.filter((op) => op.placeId !== target.id)
      }
    }
    this.changed()
    try {
      const reply = decodeReply(await o.link.call(method, body, 20_000), o.hub)
      if (!this.current(o)) return null
      if (pollId && this.operations.find((op) => op.id === pollId)?.phase !== 'verificando') return null
      const target = (body as { target?: { id: string } }).target
      const queryId = (body as { operationId?: string }).operationId
      if (queryId ? reply.operation?.id !== queryId : !reply.place) throw Error('workplace_response_invalid')
      if (target && reply.place && reply.place.id !== target.id) throw Error('workplace_response_invalid')
      if (method === 'donnaWorkplaceInspectStart' && (!reply.operation || reply.operation.placeId !== target?.id)) throw Error('workplace_response_invalid')
      if (method === 'donnaWorkplacePrepareConnection' && !reply.publicMaterial) throw Error('workplace_response_invalid')
      if (method === 'donnaWorkplaceRemove' && reply.removed !== true) throw Error('workplace_response_invalid')
      if (reply.place) this.places = reply.removed ? this.places.filter((p) => p.id !== reply.place!.id)
        : [...this.places.filter((p) => p.id !== reply.place!.id), reply.place]
      if (reply.publicMaterial) this.material = reply.publicMaterial
      if (reply.operation) {
        this.projectionEpoch++
        const place = this.places.find((p) => p.id === reply.operation!.placeId)
        if (!place || place.admissionRevision !== reply.operation.admissionRevision || place.connectionRevision !== reply.operation.connectionRevision) throw Error('operation_invalidated')
        // Cancelamento terminal não volta a verificando com um poll antigo.
        if (this.operation?.id !== reply.operation.id || this.operation.phase === 'verificando' || reply.operation.phase !== 'verificando') this.operation = reply.operation
        this.operations = [...this.operations.filter((op) => op.placeId !== reply.operation!.placeId), reply.operation]
      }
      if (reply.removed || (method !== 'donnaWorkplacePrepareConnection' && method !== 'donnaWorkplaceInspectStatus' && method !== 'donnaWorkplaceInspectCancel')) this.material = null
      return reply
    } catch (error) {
      if (pollId && this.operations.find((op) => op.id === pollId)?.phase !== 'verificando') return null
      if (this.current(o)) { this.error = errorMessage(error); if (this.error === localOnly || this.error === invalidResponse) this.refuse(error) }
      return null
    } finally { if (this.current(o)) { if (busy) this.busy = false; this.changed() } }
  }
}
export type WorkplaceError = 'soDireto' | 'respostaInvalida' | 'conflito' | 'cofre' | 'manifesto' | 'identidadeUsada' | 'generico' | 'semRecurso'
const localOnly: WorkplaceError = 'soDireto'
const invalidResponse: WorkplaceError = 'respostaInvalida'
function metodoDesconhecido(error: unknown) {
  return error instanceof Error && /^(unknown[ _]method|method_not_found)\b/i.test(error.message.trim())
}
export function errorMessage(error: unknown): WorkplaceError {
  const code = error instanceof Error ? error.message : ''
  if (code.includes('origin_not_admitted')) return localOnly
  if (/workplace_(hub_changed|response_invalid)|operation_invalidated|receipt_invalidated/.test(code)) return invalidResponse
  if (/revision_conflict|request_conflict|inspection_conflict/.test(code)) return 'conflito'
  if (/vault_unavailable/.test(code)) return 'cofre'
  if (/artifact|driver/.test(code)) return 'manifesto'
  if (/runner_pin_reserved/.test(code)) return 'identidadeUsada'
  return 'generico'
}
