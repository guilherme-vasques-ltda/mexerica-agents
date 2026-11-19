import { levelOf, microphone } from '../../call/voice'
import { ErroComChave } from '../bot-settings/mensagem'

export interface TrechoOuvido { item: string; indice: number; ms: number }
export interface AudioTempoReal {
  iniciar(entrada: (pcm: Uint8Array, nivel: number) => void): Promise<void>
  tocar(pcm: Uint8Array, item: string, indice: number): void
  cortar(): TrechoOuvido | null
  pararEntrada(): void
  terminar(): void
  readonly ocupado: boolean
  aoDrenar: (() => void) | null
}

/** Conversor contínuo: mantém a fase entre quadros, inclusive quando o dispositivo não aceita 24k. */
export class PCM24 {
  private amostras: number[] = []
  private posicao = 0
  constructor(private readonly origem: number) {
    if (!Number.isFinite(origem) || origem < 8000 || origem > 192000) throw new ErroComChave('voz.chamada.falha.audioFrequencia')
  }
  converter(entrada: Float32Array): Uint8Array {
    this.amostras.push(...entrada)
    const saida: number[] = []
    const passo = this.origem / 24000
    while (this.posicao + 1 < this.amostras.length) {
      const i = Math.floor(this.posicao)
      const parte = this.posicao - i
      const valor = Math.max(-1, Math.min(1, this.amostras[i]! * (1 - parte) + this.amostras[i + 1]! * parte))
      saida.push(Math.round(valor * (valor < 0 ? 32768 : 32767)))
      this.posicao += passo
    }
    const consumidas = Math.min(Math.floor(this.posicao), this.amostras.length)
    this.amostras.splice(0, consumidas)
    this.posicao -= consumidas
    const pcm = new Uint8Array(saida.length * 2)
    const view = new DataView(pcm.buffer)
    saida.forEach((v, i) => view.setInt16(i * 2, v, true))
    return pcm
  }
}

/** PCM mono LE, sem arquivo, sem WAV e sem IPC. No máximo dez segundos agendados. */
export class AudioPCM implements AudioTempoReal {
  aoDrenar: (() => void) | null = null
  private contexto: AudioContext | null = null
  private stream: MediaStream | null = null
  private entrada: MediaStreamAudioSourceNode | null = null
  private processador: ScriptProcessorNode | null = null
  private fontes: { no: AudioBufferSourceNode; inicio: number; duracao: number; item: string; indice: number; base: number }[] = []
  private fim = 0
  private ultimo: { item: string; indice: number; total: number } | null = null
  private encerrado = false

  get ocupado() { return this.fontes.length > 0 }

  async iniciar(entrada: (pcm: Uint8Array, nivel: number) => void) {
    const stream = await microphone()
    if (this.encerrado) { stream.getTracks().forEach(t => t.stop()); return }
    this.stream = stream
    const ctx = new AudioContext({ sampleRate: 24000 })
    this.contexto = ctx
    const conversor = new PCM24(ctx.sampleRate)
    this.entrada = ctx.createMediaStreamSource(stream)
    this.processador = ctx.createScriptProcessor(2048, 1, 1)
    this.processador.onaudioprocess = e => {
      if (this.encerrado) return
      const amostras = e.inputBuffer.getChannelData(0)
      let soma = 0
      for (const v of amostras) soma += v * v
      entrada(conversor.converter(amostras), levelOf(soma, amostras.length))
    }
    this.entrada.connect(this.processador)
    this.processador.connect(ctx.destination)
    if (ctx.state === 'suspended') await ctx.resume()
  }

  tocar(pcm: Uint8Array, item: string, indice: number) {
    const ctx = this.contexto
    if (!ctx || this.encerrado) return
    if (!pcm.length || pcm.length % 2 || pcm.length > 480_000 || Math.max(ctx.currentTime, this.fim) - ctx.currentTime + pcm.length / 48000 > 10) throw new ErroComChave('voz.chamada.falha.audioFila')
    const buffer = ctx.createBuffer(1, pcm.length / 2, 24000)
    const canal = buffer.getChannelData(0)
    const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength)
    for (let i = 0; i < canal.length; i++) canal[i] = view.getInt16(i * 2, true) / 32768
    const no = ctx.createBufferSource()
    no.buffer = buffer
    no.connect(ctx.destination)
    const inicio = Math.max(ctx.currentTime, this.fim)
    const base = this.ultimo?.item === item && this.ultimo.indice === indice ? this.ultimo.total : 0
    const trecho = { no, inicio, duracao: pcm.length / 48000, item, indice, base }
    this.ultimo = { item, indice, total: base + trecho.duracao }
    this.fim = inicio + trecho.duracao
    this.fontes.push(trecho)
    no.onended = () => {
      no.disconnect()
      this.fontes = this.fontes.filter(f => f !== trecho)
      if (!this.fontes.length && !this.encerrado) this.aoDrenar?.()
    }
    no.start(inicio)
  }

  cortar(): TrechoOuvido | null {
    const agora = this.contexto?.currentTime ?? 0
    const atual = this.fontes.find(f => f.inicio + f.duracao > agora) ?? this.fontes.at(-1)
    const ouvido = atual ? { item: atual.item, indice: atual.indice, ms: Math.floor((atual.base + Math.max(0, Math.min(atual.duracao, agora - atual.inicio))) * 1000) } : null
    const fontes = this.fontes
    this.fontes = []
    for (const f of fontes) { f.no.onended = null; f.no.stop(); f.no.disconnect() }
    this.fim = agora
    this.ultimo = null
    return ouvido
  }

  pararEntrada() {
    if (this.processador) { this.processador.onaudioprocess = null; this.processador.disconnect() }
    this.entrada?.disconnect()
    this.stream?.getTracks().forEach(t => t.stop())
    this.processador = null
    this.entrada = null
    this.stream = null
  }

  terminar() {
    if (this.encerrado) return
    this.encerrado = true
    this.cortar()
    this.pararEntrada()
    void this.contexto?.close().catch(() => {})
    this.contexto = null
    this.stream = null
    this.entrada = null
    this.processador = null
    this.aoDrenar = null
  }
}
