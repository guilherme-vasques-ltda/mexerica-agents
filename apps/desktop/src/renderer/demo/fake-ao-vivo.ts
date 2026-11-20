// Só QA/captura: um "Hub" em memória que fala o contrato browserStream* do canal protegido, com a página
// fictícia desenhada num canvas e os quadros passando pelo codec binário real (MXJ1) antes de chegar ao painel.
import type { BrowserLiveBridge, FramesHandle } from '@shared/fork-browser-live'
import type { ForkCloudCallResult, ForkCloudStreamError } from '@shared/fork-cloud'
import { decodeFrame, encodeFrame, type BrowserFrame } from '@shared/relay-protocol/browser-frames'

const VIEWPORT = { width: 1280, height: 800 }
const IMAGEM = { width: 960, height: 600 }

export type CenarioAoVivo = 'agente' | 'lento' | 'negado' | 'semcaps'

async function jpeg(desenho: (ctx: CanvasRenderingContext2D) => void): Promise<Uint8Array> {
  const canvas = document.createElement('canvas')
  canvas.width = IMAGEM.width
  canvas.height = IMAGEM.height
  const ctx = canvas.getContext('2d')!
  ctx.scale(IMAGEM.width / VIEWPORT.width, IMAGEM.height / VIEWPORT.height)
  desenho(ctx)
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('jpeg'))), 'image/jpeg', 0.72))
  return new Uint8Array(await blob.arrayBuffer())
}

/** Loja Exemplo: dados fictícios, nenhum site real. */
function pagina(ctx: CanvasRenderingContext2D, marcas: { x: number; y: number }[], tick: number) {
  ctx.fillStyle = '#f6f4ef'
  ctx.fillRect(0, 0, VIEWPORT.width, VIEWPORT.height)
  ctx.fillStyle = '#ffffff'
  ctx.fillRect(0, 0, VIEWPORT.width, 84)
  ctx.fillStyle = '#e8590c'
  ctx.beginPath()
  ctx.arc(64, 42, 20, 0, Math.PI * 2)
  ctx.fill()
  ctx.fillStyle = '#26211c'
  ctx.font = '600 28px sans-serif'
  ctx.fillText('Loja Exemplo', 100, 52)
  ctx.font = '20px sans-serif'
  ctx.fillStyle = '#6f665c'
  for (const [i, rotulo] of ['Pedidos', 'Clientes', 'Relatórios'].entries()) ctx.fillText(rotulo, 760 + i * 170, 50)
  ctx.fillStyle = '#26211c'
  ctx.font = '600 40px sans-serif'
  ctx.fillText('Pedidos recentes', 64, 170)
  for (let i = 0; i < 4; i += 1) {
    const y = 210 + i * 130
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(64, y, 1152, 108)
    ctx.strokeStyle = '#e3ddd2'
    ctx.strokeRect(64, y, 1152, 108)
    ctx.fillStyle = '#26211c'
    ctx.font = '600 26px sans-serif'
    ctx.fillText(`Pedido #${4810 + i}`, 92, y + 48)
    ctx.font = '22px sans-serif'
    ctx.fillStyle = '#6f665c'
    ctx.fillText(['Camiseta lisa · 2 un.', 'Caneca de feltro · 1 un.', 'Caderno A5 · 3 un.', 'Boné laranja · 1 un.'][i]!, 92, y + 82)
    ctx.fillStyle = i === 1 ? '#e8590c' : '#2e7d32'
    ctx.fillRect(1000, y + 34, 188, 44)
    ctx.fillStyle = '#ffffff'
    ctx.font = '600 22px sans-serif'
    ctx.fillText(i === 1 ? 'Reembolsar' : 'Concluído', 1030, y + 64)
  }
  ctx.fillStyle = '#9b9286'
  ctx.font = '18px sans-serif'
  ctx.fillText(`atualizado ${tick}`, 64, 780)
  for (const m of marcas) {
    ctx.strokeStyle = '#1971c2'
    ctx.lineWidth = 4
    ctx.beginPath()
    ctx.arc(m.x, m.y, 26, 0, Math.PI * 2)
    ctx.stroke()
    ctx.fillStyle = 'rgba(25,113,194,0.25)'
    ctx.fill()
  }
}

/** Mesma conversão do Hub (stream/mod.rs map_point): caixa inteira → viewport, margens recusadas. */
function pontoNaViewport(width: number, height: number, x: number, y: number) {
  const escala = Math.min(width / IMAGEM.width, height / IMAGEM.height)
  const mostradoL = IMAGEM.width * escala
  const mostradoA = IMAGEM.height * escala
  const esquerda = (width - mostradoL) / 2
  const topo = (height - mostradoA) / 2
  if (x < esquerda || y < topo || x >= esquerda + mostradoL || y >= topo + mostradoA) return null
  return { x: ((x - esquerda) / mostradoL) * VIEWPORT.width, y: ((y - topo) / mostradoA) * VIEWPORT.height }
}

export class HubNavegadorFalso {
  readonly chamadas: { method: string; body: unknown }[] = []
  readonly cliques: { x: number; y: number }[] = []
  private readonly cenario: CenarioAoVivo
  private sink: { onFrame: (f: BrowserFrame, waitedMs: number) => void; onEnd: (e: ForkCloudStreamError | null) => void } | null = null
  private sequence = 0
  private epoch = 0
  private controller: 'agent' | 'person' = 'agent'
  private ultimaEntrada = 0
  private emitindo = false
  private timer: ReturnType<typeof setInterval> | null = null
  private mostrado = 0
  private pendente: BrowserFrame | null = null
  private ocupado = false

  constructor(cenario: CenarioAoVivo) {
    this.cenario = cenario
  }

  readonly bridge: BrowserLiveBridge = {
    call: async (method, body) => this.chamar(method, body),
    subscribeFrames: (onFrame, onEnd) => {
      if (this.cenario === 'semcaps') queueMicrotask(() => onEnd({ status: 501, message: '' }))
      this.sink = { onFrame, onEnd }
      const handle: FramesHandle = { cancel: () => this.parar(), consumed: () => this.liberar(), opened: Promise.resolve(this.cenario !== 'semcaps') }
      return handle
    },
  }

  private liberar() {
    this.ocupado = false
    const proximo = this.pendente
    this.pendente = null
    if (proximo) this.entregar(proximo)
  }

  private entregar(frame: BrowserFrame) {
    if (this.ocupado) {
      this.pendente = frame
      return
    }
    this.ocupado = true
    this.sink?.onFrame(frame, 0)
  }

  private parar() {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
    this.sink = null
  }

  perder() {
    this.sink?.onEnd({ status: 0, message: '' })
    this.parar()
  }

  private async emitir() {
    if (this.emitindo) return
    this.emitindo = true
    try {
      this.mostrado += 1
      const bytes = await jpeg((ctx) => pagina(ctx, this.cliques.slice(-3), this.mostrado))
      this.sequence += 1
      // O quadro atravessa o codec binário real, como no canal protegido.
      const frame = decodeFrame(encodeFrame({ session: 's-qa', target: 'aba-1', sequence: this.sequence, targetGeneration: 0, image: IMAGEM, viewport: VIEWPORT, jpeg: bytes }))
      if (this.cenario !== 'lento') this.entregar(frame)
      else if (this.sequence === 1) this.entregar(frame)
    } finally {
      this.emitindo = false
    }
  }

  private chamar(method: string, body: unknown): ForkCloudCallResult {
    this.chamadas.push({ method, body })
    const b = (body ?? {}) as Record<string, number | string>
    if (method === 'browserStreamSubscribe') {
      if (this.cenario === 'negado') return { kind: 'err', status: 403, message: 'x', code: 'browser_scope_unavailable' }
      if (this.cenario === 'semcaps') return { kind: 'ok', value: { subscribed: true, frames: false, session: b.session } }
      void this.emitir()
      this.timer = setInterval(() => void this.emitir(), this.cenario === 'lento' ? 60_000 : 1500)
      return { kind: 'ok', value: { subscribed: true, frames: true, session: b.session } }
    }
    if (method === 'browserStreamControl') {
      this.controller = b.controller === 'person' ? 'person' : 'agent'
      this.epoch += 1
      return { kind: 'ok', value: { epoch: this.epoch } }
    }
    if (method === 'browserStreamAck') return { kind: 'ok', value: { acknowledged: true } }
    if (method === 'browserStreamInput') {
      const ponto = pontoNaViewport(Number(b.displayWidth), Number(b.displayHeight), Number(b.x), Number(b.y))
      const valida = ponto && b.sequence === this.ultimaEntrada + 1 && b.epoch === this.epoch && b.controller === this.controller
      if (!valida) return { kind: 'ok', value: { accepted: false, sequence: b.sequence } }
      this.ultimaEntrada = Number(b.sequence)
      this.cliques.push(ponto)
      void this.emitir()
      return { kind: 'ok', value: { accepted: true, sequence: b.sequence } }
    }
    return { kind: 'err', status: 400, message: 'x' }
  }
}
