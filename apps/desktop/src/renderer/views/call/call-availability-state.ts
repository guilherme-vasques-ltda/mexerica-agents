import type { EstadoChaves } from '../../../shared/fork-voz'
import type { HostClient } from '../../client/host-client'
import { Observable } from '../../lib/observable'
import { voiceKeyChanges, voiceStatus, type VoiceProvider, type VoiceStatus } from '../settings/voice-settings'
import { canStartCall } from './call-availability'

interface Source {
  readonly connectedClient: HostClient | null
  readonly principalEpoch: number
  subscribe(listener: () => void): () => void
}

/** Observa o link real e as gravações de voz, sem depender de um render intermediário. */
export class CallAvailabilityState extends Observable {
  private active = false
  private local = false
  private generation = 0
  private client: HostClient | null = null
  private epoch = -1
  private revision = -1
  private status: VoiceStatus | null = null
  private offKeys: (() => void) | null = null

  constructor(private readonly source: Source) { super() }

  start(local: boolean) {
    this.active = true
    this.local = local
    const offSource = this.source.subscribe(this.sync)
    this.refresh()
    return () => {
      this.active = false
      this.generation++
      this.status = null
      offSource()
      this.offKeys?.()
      this.offKeys = null
      this.client = null
    }
  }

  private sync = () => {
    const client = this.source.connectedClient
    if (client !== this.client || this.epoch !== this.source.principalEpoch ||
        (client && this.revision !== voiceKeyChanges(client).revision)) this.refresh()
  }

  private current(client: HostClient, epoch: number, revision: number) {
    return this.active && this.source.connectedClient === client && this.source.principalEpoch === epoch &&
      voiceKeyChanges(client).revision === revision && voiceKeyChanges(client).pending === 0
  }

  refresh = () => {
    if (!this.active) return
    const generation = ++this.generation
    const client = this.source.connectedClient
    if (client !== this.client) {
      this.offKeys?.()
      this.offKeys = client ? voiceKeyChanges(client).subscribe(this.sync) : null
    }
    this.client = client
    const epoch = this.epoch = this.source.principalEpoch
    const revision = this.revision = client ? voiceKeyChanges(client).revision : -1
    this.status = null
    this.changed()
    if (!client || this.local || voiceKeyChanges(client).pending > 0) return
    void voiceStatus(client).then((status) => {
      if (generation !== this.generation || !this.current(client, epoch, revision)) return
      this.status = status
      this.changed()
    }).catch(() => {
      // Consulta pendente ou falha nunca confirma voz em nuvem.
    })
  }

  available(local: boolean, provider: VoiceProvider | null, aparelho?: EstadoChaves['chaves'] | null) {
    const client = this.source.connectedClient
    if (!client) return false
    const status = this.client === client && this.current(client, this.epoch, this.revision) ? this.status : null
    return canStartCall(local, provider, status, aparelho)
  }
}
