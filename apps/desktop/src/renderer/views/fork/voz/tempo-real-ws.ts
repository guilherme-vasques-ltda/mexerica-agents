import { idSessaoXai } from '../../../../shared/fork-voz-xai.ts'
import { Despedida } from './encerrar'
import { OrigemEncerramento } from './operador-voz'
import { OPERATOR_TOOLS, type CallOperator } from '../../call/call-operator'
import { fromBase64, toBase64, VoiceEngine } from '../../call/voice'
import { openAIStartUpdates, openAIToolOutput, VoiceUpdates } from '../../call/voice-updates.ts'
import { AudioPCM, type AudioTempoReal } from './audio-pcm'
import { falhaSegura } from './falha-segura'

export type SocketTempoReal = Pick<WebSocket, 'readyState' | 'bufferedAmount' | 'send' | 'close' | 'onopen' | 'onmessage' | 'onerror' | 'onclose'>
export interface PortasTempoReal {
  audio?: AudioTempoReal
  socket?: (url: string, protocolos: string[]) => SocketTempoReal
}
export interface CredencialWS {
  secret: string
  conferir?: (sessao: unknown) => Promise<boolean>
  fechar?: () => void
}
export interface AdaptadorWS {
  provider: 'xai'
  url: string
  protocolos: (secret: string) => string[]
  confere: (sessao: unknown) => boolean
  /** xAI configura a sessão após abrir o socket. */
  sessaoInicial?: object
  iniciarEspera?: () => { type: string; response: object }
}

type ChamadaPendente = { id: string; nome: string; args: Record<string, unknown>; invalida?: true }
type Resposta = { acabou: boolean; processando: boolean; cancelada: boolean; pendentes: ChamadaPendente[] }

/** Núcleo do voz-13: transporte, PCM, fila e guarda existentes, com adaptação estreita por provedor. */
export class TempoRealWS extends VoiceEngine {
  readonly provider: 'xai'
  private conferirRemoto: CredencialWS['conferir']
  private fecharRemoto: CredencialWS['fechar']
  private configurou = false
  private conferindo = false
  private sessaoId: string | null = null
  private audioEvento: string | null = null
  private socket: SocketTempoReal | null = null
  private readonly audio: AudioTempoReal
  private readonly abrirSocket: NonNullable<PortasTempoReal['socket']>
  private readonly origem = new OrigemEncerramento()
  private readonly despedida: Despedida
  private updates = new VoiceUpdates()
  private readonly respostas = new Map<string, Resposta>()
  private readonly chamadas = new Map<string, { resposta: string; item: string }>()
  private resposta: string | null = null
  private espera: string | null = null
  private gerando = false
  private pessoaFalando = false
  private falaIniciada = false
  private entradaPendente = false
  private precisaArmar = true
  private rearmRequested = false
  private readonly pararConexao: () => void
  private precisaResponder = false
  private iniciou = false
  private pronta = false
  private tamanhoFila = 0
  private abertura: ReturnType<typeof setTimeout> | null = null
  private geracaoTimer: ReturnType<typeof setTimeout> | null = null
  private readonly inicio = Date.now()
  private teto: ReturnType<typeof setTimeout> | null = null

  constructor(
    private readonly adaptador: AdaptadorWS,
    private readonly credencial: () => Promise<CredencialWS>,
    private readonly op: CallOperator,
    portas: PortasTempoReal = {},
  ) {
    super()
    this.provider = adaptador.provider
    this.audio = portas.audio ?? new AudioPCM()
    this.abrirSocket = portas.socket ?? ((url, protocolos) => new WebSocket(url, protocolos))
    this.pararConexao = this.op.voz.aoReconectar(() => {
      this.rearmRequested = false
      this.flush()
    })
    this.despedida = new Despedida(() => {
      this.entradaPendente = false
      this.pessoaFalando = false
      this.audio.pararEntrada()
      this.origem.invalidar()
      this.onActivityChanged?.(false)
      this.onClosing?.()
    }, () => { this.end(); this.onEndCall?.() })
    this.audio.aoDrenar = () => {
      if (this.despedida.fechando) return this.despedida.audioTerminou()
      if (this.ended) return
      this.setPhase('listening')
      this.flush()
    }
  }

  async start() {
    if (this.ended || this.iniciou) return
    this.iniciou = true
    // O teto também protege o motor montado pelo roteiro Node, sem CallView.
    this.teto = setTimeout(() => this.end(), Math.max(0, 1_800_000 - (Date.now() - this.inicio)))
    this.abertura = setTimeout(() => this.falhar('rede'), 15_000)
    try {
      await this.audio.iniciar((pcm, nivel) => {
        this.level = this.muted ? 0 : nivel
        if (!this.muted && pcm.length && this.pronta && !this.despedida.fechando) this.enviar({ type: 'input_audio_buffer.append', audio: toBase64(pcm) })
      })
      if (this.ended) return this.audio.terminar()
      const auth = await this.credencial()
      if (this.ended) { auth.fechar?.(); return }
      this.conferirRemoto = auth.conferir
      this.fecharRemoto = auth.fechar
      const ws = this.abrirSocket(this.adaptador.url, this.adaptador.protocolos(auth.secret))
      this.socket = ws
      ws.onopen = () => {
        if (!this.vigente() || !this.adaptador.sessaoInicial) return
        this.configurou = true
        this.enviar({ type: 'session.update', session: this.adaptador.sessaoInicial })
      }
      ws.onmessage = e => {
        if (!this.vigente()) return
        try { this.receber(e.data) } catch { this.falhar('provedor') }
      }
      ws.onerror = () => this.falhar('rede')
      ws.onclose = () => { if (!this.ended) this.falhar('rede') }
    } catch (e) { if (!this.ended) this.fail(falhaSegura(e)) }
  }

  end() {
    if (this.ended) return
    this.ended = true
    this.pararConexao()
    if (this.abertura) clearTimeout(this.abertura)
    if (this.geracaoTimer) clearTimeout(this.geracaoTimer)
    if (this.teto) clearTimeout(this.teto)
    this.despedida.cancelar()
    this.fecharRemoto?.()
    this.fecharRemoto = undefined
    this.conferirRemoto = undefined
    this.origem.invalidar()
    const ws = this.socket
    this.socket = null
    if (ws) { ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null; ws.close() }
    this.audio.terminar()
    this.updates.cancel()
    this.updates = new VoiceUpdates()
    this.tamanhoFila = 0
    this.respostas.clear()
    this.chamadas.clear()
    this.level = 0
    this.onActivityChanged?.(false)
  }

  interrupt(cancelarNoProvedor = true) {
    if (!this.vigente()) return
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    this.op.voz.abandonarEntregas()
    this.origem.invalidar()
    this.entradaPendente = false
    if (this.resposta) {
      const r = this.respostas.get(this.resposta)
      if (r && (!r.acabou || r.processando) && !r.cancelada) {
        r.cancelada = true
        r.pendentes = []
        this.op.voz.cancelarResposta(this.resposta)
        // A geração já terminou no provedor; uma RPC antiga não segura a fila local.
        if (r.processando) { r.processando = false; this.gerando = false }
        if (cancelarNoProvedor && !r.acabou) this.enviar({ type: 'response.cancel', response_id: this.resposta })
        if (this.espera && this.chamadas.get(this.espera)?.resposta === this.resposta) { this.updates.cancel([this.espera]); this.espera = null }
      }
    }
    const trecho = this.audio.cortar()
    if (trecho) this.enviar({ type: 'conversation.item.truncate', item_id: trecho.item, content_index: trecho.indice, audio_end_ms: trecho.ms })
    if (this.pronta) this.setPhase('listening')
    // response.done pode ter chegado antes do fim da reprodução, sem outro evento para liberar a fila.
    this.flush()
  }

  speak(texto: string) { this.adicionar(this.op.reply(texto)) }
  announce(texto: string) { this.adicionar(this.op.notice(texto)) }

  private adicionar(texto: string) {
    if (!this.vigente() || this.despedida.fechando) return
    if (texto.length > 32_000 || this.tamanhoFila + texto.length > 64_000) return this.falhar('provedor')
    this.tamanhoFila += texto.length
    this.rearmRequested = false
    this.origem.invalidar()
    this.entradaPendente = false
    this.updates.push(texto)
    this.precisaArmar = true
    this.flush()
  }

  private vigente() {
    if (this.ended) return false
    if (Date.now() - this.inicio >= 1_800_000) { this.end(); return false }
    return true
  }
  private falhar(categoria: 'rede' | 'provedor') {
    if (this.despedida.fechando) return this.despedida.audioTerminou()
    if (!this.ended) this.fail(falhaSegura(categoria === 'rede' ? 'network' : 'provedor'))
  }
  private enviar(evento: object) {
    if (!this.vigente() || this.socket?.readyState !== 1) return
    if (this.socket.bufferedAmount > 262_144) return this.falhar('rede')
    try { this.socket.send(JSON.stringify(evento)) } catch { this.falhar('rede') }
  }

  private receber(data: unknown) {
    if (typeof data !== 'string' || data.length > 1_048_576) return this.falhar('provedor')
    const m = JSON.parse(data) as Record<string, unknown>
    if (!m || typeof m !== 'object' || Array.isArray(m)) return this.falhar('provedor')
    if (this.despedida.fechando) return
    if (m.type === 'conversation.created' && !this.pronta) return
    if (!this.pronta && m.type !== 'session.created' && m.type !== 'session.updated' && m.type !== 'error') return this.falhar('provedor')
    switch (m.type) {
      case 'session.created':
      case 'session.updated': {
        const s = m.session as { model?: unknown; id?: unknown } | undefined
        if (!s || s.model !== 'grok-voice-3.0' || !idSessaoXai(s.id) ||
          (this.sessaoId !== null && this.sessaoId !== s.id)) return this.falhar('provedor')
        if (m.type === 'session.created') {
          if (this.sessaoId !== null) return this.falhar('provedor')
          this.sessaoId = s.id
          return
        }
        if (!this.configurou || this.sessaoId === null || this.conferindo || !this.adaptador.confere(m.session) || !this.conferirRemoto) return this.falhar('provedor')
        if (this.pronta) this.abertura = setTimeout(() => this.falhar('rede'), 15_000)
        this.pronta = false
        this.conferindo = true
        void this.conferirRemoto(m.session).then(ok => {
          this.conferindo = false
          if (!this.vigente()) return
          if (!ok) return this.falhar('provedor')
          this.ficarPronta()
        }).catch(() => this.falhar('provedor'))
        return
      }
      case 'input_audio_buffer.speech_started':
        this.rearmRequested = false
        this.falaIniciada = !this.muted
        this.pessoaFalando = !this.muted
        // Bloquear a fila antes do corte. O VAD já cancela no provedor; aqui só corta/trunca.
        this.interrupt(false)
        break
      case 'input_audio_buffer.speech_stopped': this.pessoaFalando = false; this.level = 0; break
      case 'input_audio_buffer.committed':
        if (this.falaIniciada && !this.muted && typeof m.item_id === 'string') {
          this.entradaPendente = true
          this.origem.entrada()
          this.onUserSpoke?.()
        }
        this.falaIniciada = false
        break
      case 'response.created': {
        const r = m.response as { id?: unknown; metadata?: { vozOrigem?: unknown } } | undefined
        if (typeof r?.id !== 'string' || !r.id || this.respostas.has(r.id)) return
        this.op.voz.abandonarEntregas()
        if (this.respostas.size >= 512) return this.falhar('provedor')
        const ferramenta = this.geracaoInterna || r.metadata?.vozOrigem === 'ferramenta'
        this.geracaoInterna = false
        this.audioEvento = null
        this.respostas.set(r.id, { acabou: false, processando: false, cancelada: false, pendentes: [] })
        this.origem.iniciar(r.id, ferramenta || !this.entradaPendente)
        this.entradaPendente = false
        this.resposta = r.id
        this.gerando = true
        this.armarTimeout()
        break
      }
      case 'response.done': {
        const r = m.response as { id?: unknown; status?: unknown } | undefined
        if (typeof r?.id !== 'string') return
        const resposta = this.respostas.get(r.id)
        if (!resposta || resposta.acabou) return
        resposta.acabou = true
        const pendentes = resposta.pendentes.splice(0)
        if (this.resposta !== r.id) return
        if (this.geracaoTimer) clearTimeout(this.geracaoTimer)
        let trabalho: Promise<void> | undefined
        resposta.processando = true
        if (r.status === 'completed' && !resposta.cancelada) {
          // Aguarde a RPC antes do próximo efeito: o primeiro resultado revoga a origem.
          const misto = pendentes.length > 1
          for (const chamada of pendentes) {
            const executar = () => {
              if (!this.vigente() || this.despedida.fechando || resposta.cancelada || this.resposta !== r.id) return
              return this.executar(r.id as string, chamada, misto)
            }
            trabalho = trabalho ? trabalho.then(executar) : executar()
          }
        } else {
          resposta.cancelada = true
          this.op.voz.cancelarResposta(r.id)
          if (this.espera && this.chamadas.get(this.espera)?.resposta === r.id) {
            this.updates.cancel([this.espera]); this.espera = null
          }
        }
        const terminar = () => {
          if (!resposta.processando || this.resposta !== r.id || this.ended) return
          resposta.processando = false
          this.origem.terminar(r.id)
          this.gerando = false
          if (r.status === 'failed') return this.falhar('provedor')
          this.flush()
        }
        if (trabalho) void trabalho.then(terminar).catch(() => {
          if (resposta.processando && this.resposta === r.id && !this.ended) this.falhar('provedor')
        })
        else terminar()
        break
      }
      case 'response.output_audio.delta':
      case 'response.audio.delta': {
        if (!this.respostaValida(m.response_id) || typeof m.delta !== 'string' || typeof m.item_id !== 'string' ||
          !Number.isInteger(m.content_index) || Number(m.content_index) < 0) return
        // A xAI publica nomes novo/legado. Uma única família por resposta evita eco duplo.
        if (this.audioEvento !== null && this.audioEvento !== m.type) return
        this.audioEvento = m.type as string
        const pcm = fromBase64(m.delta)
        this.audio.tocar(pcm, m.item_id, Number(m.content_index))
        if (this.audio.ocupado) this.setPhase('speaking')
        break
      }
      case 'response.function_call_arguments.done': this.ferramenta(m); break
      case 'error': this.falhar('provedor'); break
    }
  }

  private respostaValida(id: unknown) {
    return typeof id === 'string' && id === this.resposta && !!this.respostas.get(id) && !this.respostas.get(id)!.cancelada && !this.respostas.get(id)!.acabou
  }

  private ferramenta(m: Record<string, unknown>) {
    if (!this.respostaValida(m.response_id) || typeof m.call_id !== 'string' || !m.call_id || m.call_id.length > 256 ||
      typeof m.name !== 'string' || typeof m.item_id !== 'string' || this.chamadas.has(m.call_id)) return
    if (this.chamadas.size >= 512) return this.falhar('provedor')
    this.chamadas.set(m.call_id, { resposta: m.response_id as string, item: m.item_id })
    let args: unknown
    try { args = typeof m.arguments === 'string' ? JSON.parse(m.arguments) : null } catch { args = null }
    const schema = OPERATOR_TOOLS.find(t => t.name === m.name)?.parameters
    const objeto = args && typeof args === 'object' && !Array.isArray(args) ? args as Record<string, unknown> : null
    const valida = schema && objeto && Object.keys(objeto).every(k => Object.hasOwn(schema.properties, k) &&
      (schema.properties[k]!.type === 'integer' ? Number.isInteger(objeto[k]) : typeof objeto[k] === schema.properties[k]!.type)) &&
      (schema.required ?? []).every(k => Object.hasOwn(objeto, k))
    if (!valida) {
      this.origem.invalidar()
      this.entradaPendente = false
    }
    // Argumentos finais também chegam em respostas canceladas: nenhum efeito ou espera antes do done.
    this.respostas.get(m.response_id as string)!.pendentes.push({ id: m.call_id, nome: m.name, args: objeto ?? {}, ...(!valida ? { invalida: true as const } : {}) })
  }

  private executar(resposta: string, chamada: ChamadaPendente, misto = false): Promise<void> | undefined {
    const { id, nome, args } = chamada
    const erroConexao = this.op.voz.indisponibilidade()
    if (!chamada.invalida && nome === 'bot_updates') {
      this.rearmRequested = false
      if (!misto && !erroConexao) {
        const anterior = this.updates.wait(id)
        if (anterior) {
          this.origem.invalidar()
          this.entradaPendente = false
          this.enviar(openAIToolOutput(anterior, JSON.stringify({ messages: [] })))
        }
        this.espera = id
        return
      }
    }
    const socket = this.socket
    const entregaAtual = this.op.voz.vincularEntrega(id)
    const output = chamada.invalida ? JSON.stringify({ error: 'Pedido de ferramenta inválido.' })
      : nome === 'bot_updates' ? JSON.stringify(erroConexao ? { error: erroConexao } : { messages: [] })
      : this.op.executar(nome, args, this.origem, resposta, id, () => {
          this.despedida.pedir(this.audio.ocupado)
          return true
        })
    const entregar = (saida: string) => {
      if (!this.vigente() || this.despedida.fechando || this.socket !== socket || this.resposta !== resposta ||
        this.respostas.get(resposta)?.cancelada || !entregaAtual()) return
      this.origem.invalidar()
      this.entradaPendente = false
      this.enviar(openAIToolOutput(id, saida))
      // Erro da espera offline não inicia uma continuação automática.
      if (nome !== 'bot_updates' || !erroConexao) this.precisaResponder = true
      if (misto && nome === 'bot_updates') this.precisaArmar = true
    }
    if (typeof output === 'string') entregar(output)
    else return output.then(entregar)
  }

  private ficarPronta() {
    this.pronta = true
    if (this.abertura) clearTimeout(this.abertura)
    this.setPhase(this.audio.ocupado ? 'speaking' : 'listening')
    this.flush()
  }

  private geracaoInterna = false

  private armarTimeout() {
    if (this.geracaoTimer) clearTimeout(this.geracaoTimer)
    this.geracaoTimer = setTimeout(() => this.falhar('rede'), 30_000)
  }

  private gerar(evento: { type: string; response?: object }) {
    this.gerando = true
    this.geracaoInterna = true
    this.entradaPendente = false
    this.origem.invalidar()
    this.armarTimeout()
    this.enviar({ ...evento, response: { ...evento.response, metadata: { vozOrigem: 'ferramenta' } } })
  }

  private flush() {
    if (!this.vigente() || !this.pronta || this.despedida.fechando || this.op.voz.indisponibilidade() || this.gerando || this.pessoaFalando || this.audio.ocupado) return
    const update = this.updates.take()
    if (update) {
      this.tamanhoFila = 0
      this.espera = null
      this.origem.invalidar()
      this.entradaPendente = false
      this.enviar(openAIToolOutput(update.id, update.output))
      this.precisaResponder = true
      this.precisaArmar = true
    }
    if (this.precisaResponder) {
      this.precisaResponder = false
      return this.gerar({ type: 'response.create' })
    }
    if ((this.precisaArmar || this.updates.needsWait) && !this.espera && !this.rearmRequested) {
      this.precisaArmar = false
      this.rearmRequested = true
      this.gerar(this.adaptador.iniciarEspera?.() ?? openAIStartUpdates())
    }
  }
}
