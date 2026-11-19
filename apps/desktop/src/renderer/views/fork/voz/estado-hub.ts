import type { HostClient } from '../../../client/host-client'
import { Observable } from '../../../lib/observable'
import { voiceKeyChanges, voiceModels, voiceStatus, type VoiceModels, type VoiceProvider, type VoiceStatus } from '../../settings/voice-settings'

// voz-03: o que cada Hub diz das próprias chaves de voz (configurada ou não) e o catálogo que a chave
// dele alcança. Um objeto por Hub: a resposta de um Hub nunca muda o estado de outro, e a tela mostra
// sempre o Hub do bot da conversa. Só estado e catálogo; a chave nunca passa por aqui.

export interface FonteHub {
  readonly connectedClient: HostClient | null
  readonly principalEpoch: number
  subscribe(listener: () => void): () => void
}

export class EstadoVozHub extends Observable {
  /** null enquanto o Hub não respondeu, ou sem conexão. */
  status: VoiceStatus | null = null
  falhou = false
  readonly modelos = new Map<VoiceProvider, VoiceModels>()
  private geracao = 0
  private observadores = 0
  private client: HostClient | null = null
  private epoch = -1
  private revisao = -1
  private parar: (() => void) | null = null
  private pararChaves: (() => void) | null = null

  constructor(private readonly fonte: FonteHub, private readonly buscar = { voiceStatus, voiceModels }) { super() }

  /** Começa a acompanhar o Hub; devolve a função que para. Vários observadores dividem o mesmo estado. */
  observar(): () => void {
    if (this.observadores++ === 0) {
      this.parar = this.fonte.subscribe(this.sync)
      this.atualizar()
    }
    let ativo = true
    return () => {
      if (!ativo) return
      ativo = false
      if (--this.observadores > 0) return
      this.geracao++
      this.parar?.()
      this.pararChaves?.()
      this.parar = this.pararChaves = null
      this.client = null
    }
  }

  private sync = () => {
    const client = this.fonte.connectedClient
    if (client !== this.client || this.epoch !== this.fonte.principalEpoch || (client && voiceKeyChanges(client).revision !== this.revisao)) this.atualizar()
  }

  atualizar = () => {
    if (this.observadores === 0) return
    const geracao = ++this.geracao
    const client = this.fonte.connectedClient
    if (client !== this.client) {
      this.pararChaves?.()
      this.pararChaves = client ? voiceKeyChanges(client).subscribe(this.sync) : null
      this.modelos.clear()
    }
    this.client = client
    this.epoch = this.fonte.principalEpoch
    this.revisao = client ? voiceKeyChanges(client).revision : -1
    this.status = null
    this.falhou = false
    this.changed()
    if (!client || voiceKeyChanges(client).pending > 0) return
    const atual = () => geracao === this.geracao && this.fonte.connectedClient === client
    void this.buscar.voiceStatus(client).then((status) => {
      if (!atual()) return
      this.status = status
      this.changed()
      for (const p of status.providers) if (p.configured) void this.buscar.voiceModels(client, p.provider).then((lista) => {
        if (!atual()) return
        this.modelos.set(p.provider, lista)
        this.changed()
      }).catch(() => {})
    }).catch(() => {
      if (!atual()) return
      this.falhou = true
      this.changed()
    })
  }
}

const porHub = new WeakMap<FonteHub, EstadoVozHub>()

/** O estado de voz deste Hub, o mesmo para todas as telas que o mostram. */
export function estadoVozHub(fonte: FonteHub): EstadoVozHub {
  let estado = porHub.get(fonte)
  if (!estado) { estado = new EstadoVozHub(fonte); porHub.set(fonte, estado) }
  return estado
}
