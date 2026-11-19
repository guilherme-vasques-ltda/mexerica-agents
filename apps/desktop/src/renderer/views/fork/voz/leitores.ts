import { clips } from '../../call/CloudSpeechEngine'
import { voiceFor } from '../../call/OnDeviceEngine'
import { SPEECH_CHUNK, VoiceSettings, type VoiceProvider } from '../../call/voice'
import { falhaSegura, PedidoCancelado } from './falha-segura'

// voz-03: quem lê a resposta em voz alta, separado de quem ouve. Permite as duas combinações novas
// da tela Voz: Voz do macOS lendo com a Voz para texto de um provedor, e o modo Fala de um provedor
// ouvindo pelo Ditado do macOS. Os motores herdados só recebem o leitor por um gancho curto.

export interface Leitor {
  /** O provedor que cobra a leitura; null na voz do sistema. */
  readonly provider: VoiceProvider | null
  /**
   * Lê o texto (já falável), cortando a leitura anterior. Chama `fim` uma vez ao terminar; se o provedor
   * recusar (chave ausente, inválida, sem saldo), chama `falha` no lugar, para o motor seguir com a voz local.
   */
  ler(texto: string, fim: () => void, falha?: (motivo: string) => void): void
  /** Corta a leitura em andamento; o `fim` pendente não é mais chamado. */
  parar(): void
}

/** A voz do sistema (Web Speech), como o motor local já fazia. Grátis, sem internet. */
export class LeitorDoSistema implements Leitor {
  readonly provider = null
  private geracao = 0

  ler(texto: string, fim: () => void) {
    const geracao = ++this.geracao
    speechSynthesis.cancel()
    const fala = new SpeechSynthesisUtterance(texto)
    const voz = voiceFor(texto)
    if (voz) {
      fala.voice = voz
      fala.lang = voz.lang
    }
    const rate = VoiceSettings.rate()
    fala.rate = rate > 0 ? Math.min(10, Math.max(0.1, rate / 0.5)) : 1
    const acabou = () => { if (geracao === this.geracao) fim() }
    fala.onend = acabou
    fala.onerror = acabou
    speechSynthesis.speak(fala)
  }

  parar() {
    this.geracao++
    speechSynthesis.cancel()
  }
}

type Trecho = { audio: Uint8Array; format: string } | null

/** A leitura do provedor pelo Hub (voiceSpeak), em trechos que cabem numa mensagem do canal. */
export class LeitorDoHub implements Leitor {
  private geracao = 0
  private player: HTMLAudioElement | null = null

  constructor(readonly provider: VoiceProvider, private readonly falar: (texto: string) => Promise<{ audio: Uint8Array; format: string }>) {}

  ler(texto: string, fim: () => void, falha?: (motivo: string) => void) {
    this.parar()
    const geracao = ++this.geracao
    const fila = clips(texto, SPEECH_CHUNK[this.provider])
    let motivo: string | null = null
    // Leitura cortada (ou chamada encerrada) não pede o próximo trecho; a falha já chega como texto seguro.
    const buscar = (t: string | undefined): Promise<Trecho> => t === undefined || geracao !== this.geracao ? Promise.resolve(null) : this.falar(t).catch((e: unknown) => {
      const falhou = falhaSegura(e)
      if (falhou instanceof PedidoCancelado) return null
      motivo ??= falhou.message
      console.error('speak: o provedor recusou a leitura')
      return null
    })
    let proximo = buscar(fila.shift())
    const tocar = () => {
      void proximo.then((trecho) => {
        if (geracao !== this.geracao) return
        if (!trecho) {
          this.player = null
          return motivo !== null && falha ? falha(motivo) : fim()
        }
        proximo = buscar(fila.shift())
        const tipo = trecho.format === 'aac' ? 'audio/aac' : `audio/${trecho.format}`
        const url = URL.createObjectURL(new Blob([trecho.audio as BlobPart], { type: tipo }))
        const player = new Audio(url)
        let feito = false
        const depois = () => {
          if (feito) return
          feito = true
          URL.revokeObjectURL(url)
          if (geracao === this.geracao) tocar()
        }
        player.onended = depois
        player.onerror = depois
        this.player = player
        void player.play().catch(depois)
      })
    }
    tocar()
  }

  parar() {
    this.geracao++
    if (this.player) {
      this.player.onended = null
      this.player.onerror = null
      this.player.pause()
      URL.revokeObjectURL(this.player.src)
    }
    this.player = null
  }
}
