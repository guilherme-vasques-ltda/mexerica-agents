// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { CallOperator } from './call-operator'
import { categoriaDe, falhaSegura } from '../fork/voz/falha-segura'
import { microphone, VoiceEngine } from './voice'
import { ErroComChave } from '../fork/bot-settings/mensagem'
import { openAIStartUpdates, openAIToolOutput, VoiceUpdates } from './voice-updates.ts'
import { Despedida } from '../fork/voz/encerrar'
import { OrigemEncerramento, resultadoVoz } from '../fork/voz/operador-voz'
import { LotesOpenAI } from '../fork/voz/lotes-openai'

/**
 * OpenAI Realtime over WebRTC, this computer ↔ OpenAI directly: the microphone is a WebRTC audio
 * track, the model's voice plays from the remote track, and events (tools, replies) go over
 * `oai-events`. The user's key stays with the host, which mints the client secret; the host
 * never sees audio.
 */
export class OpenAIRealtimeEngine extends VoiceEngine {
  readonly provider = 'openai' as const
  private pc: RTCPeerConnection | null = null
  private events: RTCDataChannel | null = null
  private stream: MediaStream | null = null
  private audio: HTMLAudioElement | null = null
  private readonly lotes = new LotesOpenAI()
  private updates = new VoiceUpdates()
  /** A forced turn gets one chance until new input or bot_updates arrives. */
  private rearmRequested = false
  private pararConexao: () => void
  private dropTimer: ReturnType<typeof setTimeout> | null = null
  private connectionAbort = new AbortController()
  private origem = new OrigemEncerramento()
  private falaIniciada = false
  private respostaEncerrando: string | null = null
  private despedida = new Despedida(() => {
    this.release()
    this.origem.invalidar()
    this.onActivityChanged?.(false)
    this.onClosing?.()
  }, () => { this.end(); this.onEndCall?.() })

  constructor(
    private credential: () => Promise<string>,
    private op: CallOperator,
  ) {
    super()
    this.pararConexao = this.op.voz.aoReconectar(() => {
      this.rearmRequested = false
      this.flush()
    })
  }

  async start() {
    if (this.ended || this.despedida.fechando) return
    try {
      this.stream = await microphone()
      if (this.ended) return this.release()
      const secret = await this.credential()
      if (this.ended) return
      await this.connect(secret)
    } catch (e) {
      if (!this.ended && !this.despedida.fechando) this.fail(e)
    }
  }

  private async connect(secret: string) {
    const pc = new RTCPeerConnection({ bundlePolicy: 'max-bundle' })
    this.pc = pc
    const track = this.stream!.getAudioTracks()[0]
    if (!track) throw new ErroComChave('voz.chamada.falha.audioNaoIniciou')
    track.enabled = !this.muted
    pc.addTrack(track, this.stream!)
    const audio = new Audio()
    audio.autoplay = true
    this.audio = audio
    pc.ontrack = (e) => {
      if (this.ended || this.despedida.fechando) return
      audio.srcObject = e.streams[0] ?? new MediaStream([e.track])
    }
    pc.oniceconnectionstatechange = () => this.iceChanged(pc.iceConnectionState)
    const events = pc.createDataChannel('oai-events')
    events.onopen = () => this.opened()
    events.onmessage = (e) => typeof e.data === 'string' && this.received(e.data)
    this.events = events

    const offer = await pc.createOffer()
    if (this.ended) return
    await pc.setLocalDescription(offer)
    if (this.ended) return
    const response = await fetch('https://api.openai.com/v1/realtime/calls', {
      method: 'POST',
      headers: { Authorization: `Bearer ${secret}`, 'Content-Type': 'application/sdp' },
      body: offer.sdp,
      signal: AbortSignal.any([this.connectionAbort.signal, AbortSignal.timeout(15_000)]),
    })
    const body = await response.text()
    if (this.ended) return
    if (!response.ok) {
      let message: string | undefined
      try {
        message = (JSON.parse(body) as { error?: { message?: string } }).error?.message
      } catch {}
      // voz-03: só a categoria segue; a mensagem da OpenAI pode ecoar a credencial.
      throw falhaSegura(`${response.status} ${message ?? ''}`)
    }
    await pc.setRemoteDescription({ type: 'answer', sdp: body })
    if (this.ended) return
    this.dropTimer = setTimeout(() => {
      if (!this.ended && this.phase === 'starting') this.fail({ chave: 'voz.chamada.falha.openaiAlcance' })
    }, 15_000)
  }

  private release() {
    for (const t of this.stream?.getTracks() ?? []) t.stop()
    this.stream = null
  }

  end() {
    this.ended = true
    this.lotes.abandonar()
    this.pararConexao()
    this.connectionAbort.abort()
    this.despedida.cancelar()
    this.origem.invalidar()
    this.updates.cancel()
    this.onActivityChanged?.(false)
    if (this.dropTimer) clearTimeout(this.dropTimer)
    this.events?.close()
    this.pc?.close()
    this.events = null
    this.pc = null
    this.release()
    if (this.audio) {
      this.audio.srcObject = null
      this.audio = null
    }
  }

  protected mutedChanged(on: boolean) {
    for (const t of this.stream?.getAudioTracks() ?? []) t.enabled = !on
    if (on) this.level = 0
  }

  interrupt() {
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    this.op.voz.abandonarEntregas()
    this.lotes.abandonar()
    this.origem.invalidar()
    if (this.phase !== 'speaking') return
    this.send({ type: 'response.cancel' })
    this.send({ type: 'output_audio_buffer.clear' })
    this.setPhase('listening')
  }

  speak(reply: string) {
    if (this.ended || this.despedida.fechando || this.phase === 'failed') return
    this.rearmRequested = false
    this.origem.invalidar()
    this.updates.push(this.op.reply(reply))
    this.flush()
  }

  announce(notice: string) {
    if (this.ended || this.despedida.fechando || this.phase === 'failed') return
    this.rearmRequested = false
    this.origem.invalidar()
    this.updates.push(this.op.notice(notice))
    this.flush()
  }

  // MARK: events

  private opened() {
    if (this.ended) return
    if (this.dropTimer) clearTimeout(this.dropTimer)
    this.send({
      type: 'session.update',
      session: {
        type: 'realtime',
        instructions: this.op.instructions,
        tools: this.op.ferramentas.map((t) => ({ type: 'function', name: t.name, description: t.description, parameters: t.parameters })),
        audio: { input: { turn_detection: { type: 'semantic_vad' } } },
      },
    })
    this.setPhase('listening')
    this.rearmRequested = true
    this.send(openAIStartUpdates())
  }

  private received(data: string) {
    if (this.ended) return
    let msg: Record<string, unknown>
    try {
      msg = JSON.parse(data) as Record<string, unknown>
    } catch {
      return
    }
    if (this.despedida.fechando) {
      if (msg.response_id === this.respostaEncerrando && (msg.type === 'output_audio_buffer.stopped' || msg.type === 'output_audio_buffer.cleared')) this.despedida.audioTerminou()
      return
    }
    switch (msg.type) {
      case 'input_audio_buffer.speech_started':
        this.op.voz.abandonarEntregas()
        this.lotes.abandonar()
        this.origem.invalidar()
        this.falaIniciada = !this.muted
        this.rearmRequested = false
        this.level = this.muted ? 0 : 0.7
        break
      // voz-03: turno da pessoa confirmado pelo provedor; conta como conversa nos limites da chamada.
      case 'input_audio_buffer.committed':
        if (this.falaIniciada && !this.muted && typeof msg.item_id === 'string') this.origem.entrada()
        this.falaIniciada = false
        this.onUserSpoke?.()
        break
      case 'input_audio_buffer.speech_stopped':
        this.level = 0
        break
      case 'output_audio_buffer.started':
        this.setPhase('speaking')
        break
      case 'output_audio_buffer.stopped':
      case 'output_audio_buffer.cleared':
        if (msg.type === 'output_audio_buffer.cleared') this.origem.invalidar()
        if (this.phase === 'speaking') this.setPhase('listening')
        break
      case 'response.created': {
        const response = msg.response as { id?: string; metadata?: { vozOrigem?: string } } | undefined
        if (typeof response?.id !== 'string' || !this.lotes.iniciar(response.id)) return
        this.op.voz.abandonarEntregas()
        this.origem.iniciar(response.id, response.metadata?.vozOrigem === 'ferramenta')
        break
      }
      case 'response.done': {
        const response = msg.response as { id?: string; status?: string } | undefined
        if (!this.lotes.terminar(response?.id, response?.status)) return
        if (response?.status !== 'completed') this.op.voz.cancelarResposta(response?.id)
        this.origem.terminar(response?.id)
        if (response?.status !== 'failed') this.flush()
        break
      }
      case 'response.function_call_arguments.done': {
        if (typeof msg.call_id !== 'string' || typeof msg.name !== 'string') return
        const concluir = this.lotes.invocar(msg.response_id, msg.call_id)
        if (!concluir) return
        if (msg.name === 'bot_updates') {
          concluir(false)
          this.rearmRequested = false
          const erro = this.op.voz.indisponibilidade()
          if (erro) { this.send(openAIToolOutput(msg.call_id, JSON.stringify({ error: erro }))); break }
          const previous = this.updates.wait(msg.call_id)
          if (previous) this.send(openAIToolOutput(previous, JSON.stringify({ messages: [] })))
          this.flush()
          break
        }
        let args: Record<string, unknown> = {}
        try {
          if (typeof msg.arguments === 'string') args = JSON.parse(msg.arguments) as Record<string, unknown>
        } catch {}
        const canal = this.events
        const entregaAtual = this.op.voz.vincularEntrega(msg.call_id)
        const output = this.op.executar(msg.name, args, this.origem, msg.response_id, msg.call_id, () => {
          this.respostaEncerrando = msg.response_id as string
          this.despedida.pedir(this.phase === 'speaking')
          return true
        })
        if (this.despedida.fechando) return
        const entregar = (item: ReturnType<typeof openAIToolOutput>) => {
          if (this.ended || this.despedida.fechando || this.events !== canal || !entregaAtual() || !concluir()) return
          this.send(item)
          this.flush()
        }
        for (const item of resultadoVoz(output, (s) => openAIToolOutput(msg.call_id as string, s), entregar)) entregar(item)
        break
      }
      case 'error':
        console.error('realtime:', categoriaDe((msg.error as { message?: string } | undefined)?.message ?? ''))
        break
    }
  }

  private iceChanged(state: RTCIceConnectionState) {
    if (this.ended) return
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    switch (state) {
      case 'connected':
      case 'completed':
        if (this.dropTimer) clearTimeout(this.dropTimer)
        break
      case 'disconnected':
        if (this.dropTimer) clearTimeout(this.dropTimer)
        this.dropTimer = setTimeout(() => !this.ended && this.fail({ chave: 'voz.chamada.falha.openaiCaiu' }), 5000)
        break
      case 'failed':
        this.fail({ chave: 'voz.chamada.falha.openaiCaiu' })
        break
    }
  }

  /** Only tool outputs enter the session; bot replies never impersonate the user. */
  private flush() {
    if (this.ended || this.despedida.fechando || this.op.voz.indisponibilidade()) return
    if (this.lotes.ocupado || !this.isActive || this.events?.readyState !== 'open') return
    const update = this.updates.take()
    if (update) {
      this.send(openAIToolOutput(update.id, update.output))
    }
    if (this.updates.needsWait) {
      if (this.rearmRequested) return
      this.rearmRequested = true
      this.send(openAIStartUpdates())
      return
    }
    if (!update && !this.lotes.continuar) return
    this.send({ type: 'response.create', response: { metadata: { vozOrigem: 'ferramenta' } } })
  }

  private send(event: Record<string, unknown>) {
    if (this.ended || this.despedida.fechando) return
    if (event.type === 'response.create') { this.lotes.solicitar(); this.op.voz.abandonarEntregas() }
    if (event.type === 'conversation.item.create' || event.type === 'response.create') this.origem.invalidar()
    if (this.events?.readyState === 'open') this.events.send(JSON.stringify(event))
  }
}
