// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { CallOperator } from './call-operator'
import { falhaSegura } from '../fork/voz/falha-segura'
import { fromBase64, micPCM16k, toBase64, VoiceEngine } from './voice'
import { geminiStartUpdates, geminiToolOutput, VoiceUpdates } from './voice-updates.ts'
import { Despedida } from '../fork/voz/encerrar'
import { OrigemGemini, resultadoVoz } from '../fork/voz/operador-voz'

const URL_BASE = 'wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained'

/** Gemini's schema spells types in capitals (`OBJECT`, `STRING`). */
function geminiSchema(schema: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(schema)) {
    if (key === 'type' && typeof value === 'string') out[key] = value.toUpperCase()
    else if (key === 'properties' && value && typeof value === 'object')
      out[key] = Object.fromEntries(Object.entries(value).map(([k, v]) => [k, geminiSchema(v as Record<string, unknown>)]))
    else out[key] = value
  }
  return out
}

/**
 * Gemini Live over a WebSocket, this computer ↔ Google directly: 16 kHz PCM up, 24 kHz PCM down,
 * echo-cancelled so you can talk over it. The user's key stays with the host, which mints one-use
 * ephemeral tokens; a session the server ends (`goAway`) is resumed with a fresh one.
 * voz-03: só o `goAway` de uma chamada ativa retoma, uma vez por aviso; qualquer outro fechamento falha.
 */
export class GeminiLiveEngine extends VoiceEngine {
  readonly provider = 'gemini' as const
  private socket: WebSocket | null = null
  /** Setup is done on the current socket. */
  private ready = false
  private resumeHandle: string | null = null
  /** voz-03: o servidor mandou `goAway` no socket atual; vale uma retomada. */
  private goAway = false
  private stopMic: (() => void) | null = null
  private output: AudioContext | null = null
  private sources = new Set<AudioBufferSourceNode>()
  private nextStart = 0
  /** Buffers scheduled and not played yet; bumping the generation drops late callbacks. */
  private queued = 0
  private playGeneration = 0
  /** The model finished its turn (nothing more is coming until we or the user speak). */
  private turnDone = true
  /** After a local interrupt, the rest of the turn's audio is dropped. */
  private dropping = false
  private updates = new VoiceUpdates()
  /** A forced turn gets one chance until new input or bot_updates arrives. */
  private rearmRequested = false
  private pararConexao: () => void
  /** Bloqueia só o rearme durante a fala; transcrição atrasada expira após silêncio. */
  private userSpoke = false
  private userSpeechTimer: ReturnType<typeof setTimeout> | null = null
  private origem = new OrigemGemini()
  private despedida = new Despedida(() => {
    this.stopMic?.()
    this.stopMic = null
    this.clearUserSpeech()
    this.origem.invalidar()
    this.onActivityChanged?.(false)
    this.onClosing?.()
  }, () => { this.end(); this.onEndCall?.() })

  constructor(
    private model: string,
    private voice: string,
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
    await this.connect()
  }

  private async connect() {
    if (this.ended || this.despedida.fechando) return
    try {
      const token = await this.credential()
      if (this.ended || this.despedida.fechando) return
      const socket = new WebSocket(`${URL_BASE}?access_token=${encodeURIComponent(token)}`)
      socket.binaryType = 'arraybuffer'
      this.socket = socket
      this.ready = false
      socket.onopen = () => this.sendSetup(socket)
      socket.onmessage = (e) => {
        if (this.socket !== socket) return
        this.received(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data as ArrayBuffer))
      }
      socket.onclose = (e) => this.closed(socket, e.reason)
    } catch (e) {
      if (!this.ended && !this.despedida.fechando) this.fail(e)
    }
  }

  private sendSetup(socket: WebSocket) {
    const tools = this.op.ferramentas.map((t) => ({
      name: t.name,
      description: t.description,
      ...(t.deferred ? { behavior: 'NON_BLOCKING' } : {}),
      ...(Object.keys(t.parameters.properties).length ? { parameters: geminiSchema(t.parameters) } : {}),
    }))
    const setup = {
      model: `models/${this.model}`,
      generationConfig: {
        responseModalities: ['AUDIO'],
        speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: this.voice } } },
      },
      systemInstruction: { parts: [{ text: this.op.instructions }] },
      tools: [{ functionDeclarations: tools }],
      inputAudioTranscription: {},
      // Audio sessions are otherwise capped at 15 minutes.
      contextWindowCompression: { slidingWindow: {} },
      sessionResumption: this.resumeHandle ? { handle: this.resumeHandle } : {},
    }
    this.send({ setup }, socket)
  }

  end() {
    this.ended = true
    this.pararConexao()
    this.despedida.cancelar()
    this.origem.invalidar()
    this.updates.cancel()
    this.clearUserSpeech()
    this.onActivityChanged?.(false)
    const socket = this.socket
    this.socket = null
    socket?.close(1000)
    this.stopPlayback()
    this.stopMic?.()
    this.stopMic = null
    void this.output?.close()
    this.output = null
  }

  protected mutedChanged(on: boolean) {
    this.level = 0
    if (on) this.send({ realtimeInput: { audioStreamEnd: true } })
  }

  interrupt() {
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    this.op.voz.abandonarEntregas()
    this.origem.invalidar()
    if (this.phase !== 'speaking') return
    this.stopPlayback()
    this.dropping = !this.turnDone
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

  // MARK: socket

  private closed(socket: WebSocket, reason: string) {
    if (this.ended || this.socket !== socket) return
    this.origem.invalidar()
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    this.socket = null
    this.ready = false
    this.clearUserSpeech()
    this.updates.cancel()
    const resume = this.goAway && this.isActive && this.resumeHandle !== null
    this.goAway = false
    if (resume) void this.connect()
    else this.fail(reason ? falhaSegura(reason) : { chave: 'voz.chamada.falha.geminiCaiu' })
  }

  private received(data: string) {
    if (this.ended) return
    let msg: Record<string, any>
    try {
      msg = JSON.parse(data) as Record<string, any>
    } catch {
      return
    }
    if (this.despedida.fechando) {
      for (const part of msg.serverContent?.modelTurn?.parts ?? []) {
        if (typeof part?.inlineData?.data === 'string') this.play(part.inlineData.data)
      }
      if (msg.serverContent?.turnComplete === true) this.turnDone = true
      if (this.turnDone && this.queued === 0) this.despedida.audioTerminou()
      return
    }
    if (msg.setupComplete !== undefined) {
      this.ready = true
      if (this.phase === 'starting') this.setPhase('listening')
      // A silent user still needs a real bot_updates call before replies can be delivered.
      this.turnDone = false
      this.rearmRequested = true
      this.send(geminiStartUpdates())
      this.origem.iniciarSessao()
      if (!this.stopMic) void this.startAudio()
    }
    const content = msg.serverContent
    if (Array.isArray(msg.toolCallCancellation?.ids)) { this.origem.invalidar(); this.op.voz.cancelar(msg.toolCallCancellation.ids) }
    if (content) {
      if (content.interrupted === true) {
        this.op.voz.abandonarEntregas()
        this.origem.invalidar()
        this.stopPlayback()
        this.dropping = false
        if (this.phase === 'speaking') this.setPhase('listening')
      }
      if (typeof content.inputTranscription?.text === 'string' && content.inputTranscription.text.trim()) {
        const antes = this.origem.versao
        this.origem.transcricao(this.turnDone || content.interrupted === true)
        if (this.origem.versao !== antes) this.op.voz.abandonarEntregas()
        this.userTranscribed()
      }
      const parts = content.modelTurn?.parts
      if (content.modelTurn) {
        if (!this.origem.emResposta) { this.op.voz.abandonarEntregas(); this.origem.iniciar('gemini') }
        this.turnDone = false
        for (const part of Array.isArray(parts) ? parts : []) {
          const audio = part?.inlineData?.data
          if (typeof audio === 'string' && !this.dropping) this.play(audio)
        }
      }
      if (content.turnComplete === true) {
        if (this.userSpoke) {
          this.clearUserSpeech()
          this.rearmRequested = false
        }
        this.turnDone = true
        this.dropping = false
        if (this.queued === 0 && this.phase === 'speaking') this.setPhase('listening')
      }
    }
    const calls = msg.toolCall?.functionCalls
    let functionResponses: ReturnType<typeof geminiToolOutput>[] = []
    if (Array.isArray(calls)) {
      functionResponses = calls
        .filter((c) => typeof c?.id === 'string' && typeof c?.name === 'string')
        .flatMap((c) => {
          if (this.despedida.fechando) return []
          if (c.name === 'bot_updates') {
            this.rearmRequested = false
            const erro = this.op.voz.indisponibilidade()
            if (erro) return [geminiToolOutput(c.id, c.name, JSON.stringify({ error: erro }))]
            const previous = this.updates.wait(c.id)
            return previous ? [geminiToolOutput(previous, c.name, JSON.stringify({ messages: [] }), 'SILENT')] : []
          }
          if (!this.origem.emResposta) { this.op.voz.abandonarEntregas(); this.origem.iniciar('gemini') }
          const socket = this.socket
          const entregaAtual = this.op.voz.vincularEntrega(c.id)
          const output = this.op.executar(c.name, (c.args as Record<string, unknown>) ?? {}, this.origem, 'gemini', c.id, () => {
            this.despedida.pedir(this.queued > 0)
            return true
          })
          return resultadoVoz(output, (s) => geminiToolOutput(c.id, c.name, s), (item) => {
            if (this.ended || this.despedida.fechando || this.socket !== socket || !entregaAtual()) return
            this.turnDone = false
            this.send({ toolResponse: { functionResponses: [item] } })
            this.flush()
          })
        })
    }
    if (content?.turnComplete === true) this.origem.terminar()
    // O fim no pacote da chamada pertence ao turno anterior ao resultado confirmar.
    if (functionResponses.length) this.send({ toolResponse: { functionResponses } })
    if (this.ended || this.despedida.fechando) return
    if (Array.isArray(msg.toolCallCancellation?.ids)) this.updates.cancel(msg.toolCallCancellation.ids)
    const update = msg.sessionResumptionUpdate
    if (update?.resumable === true && typeof update.newHandle === 'string') this.resumeHandle = update.newHandle
    // The server ends this connection soon; carry on in a resumed one.
    if (msg.goAway !== undefined && this.socket) {
      this.goAway = true
      this.socket.close(1000)
    }
    this.flush()
  }

  private send(message: Record<string, unknown>, socket?: WebSocket) {
    if (this.ended || this.despedida.fechando) return
    if (message.clientContent) this.op.voz.abandonarEntregas()
    if (message.toolResponse || message.clientContent) this.origem.invalidar()
    const target = socket ?? (this.ready ? this.socket : null)
    if (target?.readyState === WebSocket.OPEN) {
      target.send(JSON.stringify(message))
      if (message.toolResponse) this.op.voz.resultadoGemini(message, this.origem)
    }
  }

  private clearUserSpeech() {
    if (this.userSpeechTimer !== null) clearTimeout(this.userSpeechTimer)
    this.userSpeechTimer = null
    this.userSpoke = false
  }

  private userTranscribed() {
    this.clearUserSpeech()
    this.onUserSpoke?.()
    this.userSpoke = true
    // A transcrição pode chegar após turnComplete, sem outro turno para consumir a fala.
    this.userSpeechTimer = setTimeout(() => {
      this.userSpeechTimer = null
      if (this.ended || !this.ready || !this.turnDone) return
      this.userSpoke = false
      // Silêncio local não separa fragmentos; o rearme efetivamente enviado revoga.
      if (!this.origem.confirmacaoAberta) this.origem.invalidar()
      this.rearmRequested = false
      this.flush()
    }, 2500)
  }

  /** Entrega após o turno e a reprodução; o rearme também aguarda a fala. */
  private flush() {
    if (this.ended || this.despedida.fechando || this.op.voz.indisponibilidade()) return
    if (!this.ready || !this.turnDone || this.queued !== 0) return
    const update = this.updates.take()
    if (!update) {
      if (this.updates.needsWait && !this.rearmRequested && !this.userSpoke) {
        this.rearmRequested = true
        this.turnDone = false
        this.send(geminiStartUpdates())
      }
      return
    }
    this.turnDone = false
    this.send({ toolResponse: { functionResponses: [geminiToolOutput(update.id, 'bot_updates', update.output)] } })
  }

  // MARK: audio

  private async startAudio() {
    try {
      this.output = new AudioContext({ sampleRate: 24000 })
      const stop = await micPCM16k((pcm, level) => this.captured(pcm, level))
      if (this.ended || this.despedida.fechando) return stop()
      this.stopMic = stop
    } catch (e) {
      if (!this.ended && !this.despedida.fechando) this.fail(e)
    }
  }

  private captured(pcm: Int16Array, level: number) {
    if (this.ended || this.despedida.fechando || this.muted || !this.isActive) return
    if (level > 0.3) this.origem.captura()
    this.level = this.phase === 'speaking' ? 0 : level
    this.send({ realtimeInput: { audio: { data: toBase64(new Uint8Array(pcm.buffer)), mimeType: 'audio/pcm;rate=16000' } } })
  }

  private play(base64: string) {
    const ctx = this.output
    if (!ctx) return
    const bytes = fromBase64(base64)
    const frames = bytes.length >> 1
    if (!frames) return
    const view = new DataView(bytes.buffer)
    const buffer = ctx.createBuffer(1, frames, 24000)
    const out = buffer.getChannelData(0)
    for (let i = 0; i < frames; i++) out[i] = view.getInt16(i * 2, true) / 32768
    const source = ctx.createBufferSource()
    source.buffer = buffer
    source.connect(ctx.destination)
    this.nextStart = Math.max(this.nextStart, ctx.currentTime)
    source.start(this.nextStart)
    this.nextStart += buffer.duration
    this.sources.add(source)
    this.queued++
    // Sinal verificável de emissão: áudio não vazio encaminhado à reprodução após confirmar.
    this.origem.modeloFalou()
    this.level = 0
    if (this.phase !== 'speaking') this.setPhase('speaking')
    const generation = this.playGeneration
    source.onended = () => {
      this.sources.delete(source)
      this.played(generation)
    }
  }

  private played(generation: number) {
    if (generation !== this.playGeneration || this.ended) return
    this.queued = Math.max(0, this.queued - 1)
    if (this.queued !== 0 || !this.turnDone) return
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    if (this.phase === 'speaking') this.setPhase('listening')
    this.flush()
  }

  private stopPlayback() {
    this.playGeneration++
    this.queued = 0
    this.nextStart = 0
    for (const s of this.sources) {
      s.onended = null
      try {
        s.stop()
      } catch {}
    }
    this.sources.clear()
  }
}
