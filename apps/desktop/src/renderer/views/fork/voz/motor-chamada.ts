import { XaiLive, type PortasXai } from './xai-live'
import type { BotStore } from '../../../store/bot-store'
import { CallOperator } from '../../call/call-operator'
import { CloudSpeechEngine } from '../../call/CloudSpeechEngine'
import { GeminiLiveEngine } from '../../call/GeminiLiveEngine'
import { OnDeviceEngine } from '../../call/OnDeviceEngine'
import { OpenAIRealtimeEngine } from '../../call/OpenAIRealtimeEngine'
import { fromBase64, toBase64, VoiceSettings, type VoiceEngine } from '../../call/voice'
import type { VozBridge } from '../../../../shared/fork-voz'
import { falhaDaCategoria, falhaSegura, PedidoCancelado } from './falha-segura'
import { LeitorDoHub, LeitorDoSistema } from './leitores'
import type { PlanoMotor } from './plano-motor'
import type { VozProvedor } from './voz-model'

export { depoisDaFalha, nomeDoMotor, planoDeQueda, planoDoMotor, type PlanoMotor } from './plano-motor'

// voz-03: monta o motor que o plano (plano-motor.ts) escolheu, com os RPCs de voz do Hub do bot.

export interface Dependencias {
  store: BotStore
  botId: string
  send: (text: string) => void
  /** voz-04: a credencial curta feita pelo main com a chave deste aparelho (padrão: a ponte do preload). */
  credencialDoAparelho?: VozBridge['credencial']
  credencialXai?: VozBridge['credencialXai']
  conferirSessaoXai?: VozBridge['conferirSessaoXai']
  fecharSessaoXai?: VozBridge['fecharSessaoXai']
  portasXai?: PortasXai
}

type Cliente = Awaited<ReturnType<BotStore['ready']>>

/** Espera a resposta só enquanto a sessão vale; o fim da chamada solta quem espera na hora. */
function enquantoVale<T>(pedido: Promise<T>, sinal: AbortSignal): Promise<T> {
  if (sinal.aborted) return Promise.reject(new PedidoCancelado())
  return new Promise<T>((resolve, reject) => {
    const cancelar = () => reject(new PedidoCancelado())
    sinal.addEventListener('abort', cancelar, { once: true })
    pedido.then(resolve, reject).finally(() => sinal.removeEventListener('abort', cancelar))
  })
}

/**
 * Monta o motor do plano. Cada motor tem uma sessão (`AbortController`): `end()` a cancela, e todo pedido
 * ao Hub confere a sessão antes de sair e depois de cada espera. O RPC do Hub não aceita `AbortSignal`;
 * um pedido que já saiu termina lá, mas a resposta é descartada e nada novo começa depois do fim.
 * Toda falha de provedor chega ao motor já como `FalhaVoz` (categoria + texto leigo), nunca o texto bruto.
 */
export function criarMotor(plano: Exclude<PlanoMotor, { tipo: 'sem-voz' } | { tipo: 'sem-ouvir' } | { tipo: 'so-tempo-real' }>, { store, botId, send, credencialDoAparelho, credencialXai, conferirSessaoXai, fecharSessaoXai, portasXai }: Dependencias): VoiceEngine {
  const sessao = new AbortController()
  const pedido = async <T>(fazer: (c: Cliente) => Promise<T>): Promise<T> => {
    if (sessao.signal.aborted) throw new PedidoCancelado()
    try {
      const cliente = await enquantoVale(store.ready(), sessao.signal)
      if (sessao.signal.aborted) throw new PedidoCancelado()
      return await enquantoVale(fazer(cliente), sessao.signal)
    } catch (e) {
      throw falhaSegura(e)
    }
  }
  /** Como `pedido`, mas sem o Hub: o pedido vai ao main deste aparelho. */
  const doAparelho = async <T>(fazer: () => Promise<T>): Promise<T> => {
    if (sessao.signal.aborted) throw new PedidoCancelado()
    try {
      return await enquantoVale(fazer(), sessao.signal)
    } catch (e) {
      throw falhaSegura(e)
    }
  }
  const transcrever = (p: VozProvedor) => (audio: Uint8Array) =>
    pedido(async (c) => (await c.call<{ text: string }>('voiceTranscribe', { provider: p, model: VoiceSettings.transcribeModel(p), audio: toBase64(audio) }, 60_000)).text)
  const falar = (p: VozProvedor) => (text: string) =>
    pedido(async (c) => {
      const clip = await c.call<{ audio: string; format: string }>('voiceSpeak', { provider: p, model: VoiceSettings.speechModel(p), voice: VoiceSettings.voice(p), text }, 90_000)
      return { audio: fromBase64(clip.audio), format: clip.format }
    })
  const motor = montar()
  motor.onClosing = () => sessao.abort()
  const encerrar = motor.end.bind(motor)
  // Também quando o próprio motor falha: `fail()` chama `end()`.
  motor.end = () => {
    sessao.abort()
    encerrar()
  }
  return motor

  function montar(): VoiceEngine {
    switch (plano.tipo) {
      case 'tempo-real': {
        if (plano.provider === 'xai') {
          const selecao = { ...plano.selecao }
          return new XaiLive(selecao, async p => {
            // Mesmo depois de end(), o retorno do mint precisa liberar o contexto no main.
            if (plano.fonte !== 'aparelho' || sessao.signal.aborted) throw falhaDaCategoria('sem-chave')
            let fechado = false
            const fechar = (id: string) => {
              if (fechado) return
              fechado = true
              void (fecharSessaoXai ?? window.codync.voz.fecharSessaoXai)(id).catch(() => {})
            }
            const mint = (credencialXai ?? window.codync.voz.credencialXai)(p)
            // O cancelamento solta start() agora; o retorno tardio só fecha seu contexto no main.
            void mint.then(r => { if (r.ok && sessao.signal.aborted) fechar(r.negociacao) }, () => {})
            const r = await doAparelho(() => mint)
            if (!r.ok) throw falhaDaCategoria(r.categoria)
            if (sessao.signal.aborted) { fechar(r.negociacao); throw new PedidoCancelado() }
            const id = r.negociacao
            return { secret: r.credential,
              conferir: s => doAparelho(async () => (await (conferirSessaoXai ?? window.codync.voz.conferirSessaoXai)(id, s)).ok),
              fechar: () => fechar(id),
            }
          }, new CallOperator(store, botId, sessao.signal), portasXai)
        }
        const p = plano.provider
        const model = VoiceSettings.model(p)
        const voice = VoiceSettings.voice(p)
        // A fonte veio do plano: com chave deste aparelho, só o main; recusa ou cofre fechado viram falha
        // (e a queda local da chamada), nunca a reserva do Hub.
        const credential = plano.fonte === 'aparelho'
          ? () => doAparelho(async () => {
              const r = await (credencialDoAparelho ?? window.codync.voz.credencial)(p, model, voice)
              if (!r.ok) throw falhaDaCategoria(r.categoria)
              return r.credential
            })
          : () => pedido(async (c) => (await c.call<{ credential: string }>('voiceSession', { provider: p, model, voice }, 30_000)).credential)
        const op = new CallOperator(store, botId, sessao.signal)
        return p === 'openai' ? new OpenAIRealtimeEngine(credential, op) : new GeminiLiveEngine(model, voice, credential, op)
      }
      case 'fala':
        return new CloudSpeechEngine(plano.ler, { transcribe: transcrever(plano.ouvir), speak: falar(plano.ler), send })
      case 'fala-ditado':
        return new OnDeviceEngine(send, new LeitorDoHub(plano.ler, falar(plano.ler)))
      case 'local':
        return new OnDeviceEngine(send)
      case 'local-ouvindo':
        return new CloudSpeechEngine(plano.ouvir, {
          transcribe: transcrever(plano.ouvir),
          // A leitura é a Voz do macOS (LeitorDoSistema): este caminho nunca fala; o erro é só um código.
          speak: () => Promise.reject(new Error('leitura_do_sistema')),
          send,
          read: new LeitorDoSistema(),
        })
    }
  }
}
