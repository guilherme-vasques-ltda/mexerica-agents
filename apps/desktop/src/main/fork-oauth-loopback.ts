// Retorno do login de conector quando o Hub roda em OUTRO computador (túnel SSH numa porta local aleatória): o redirect
// do fornecedor não pode ser a porta do Hub, então o app abre um ouvinte próprio em 127.0.0.1 (porta fixa, uma por
// ambiente), só enquanto o login está em andamento, e entrega o retorno ao renderer, que o repassa ao Hub pelo canal
// autenticado (`connectorSignInFinish`). O Hub conferiu e guardou state, PKCE e emissor; aqui só se barra o que não é
// o retorno desta tentativa. Sem Electron neste módulo: o IPC fica em fork-oauth-loopback-ipc.ts.
import { timingSafeEqual } from 'node:crypto'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import type { AppLanguage } from '../shared/fork-idioma.ts'

/** As portas de retorno do app: principal e dev. A mesma lista vive no Hub (`host/src/fork/conectores/oauth_desktop.rs`). */
export const PORTA_PRINCIPAL = 19424
export const PORTA_DEV = 19425
export const CAMINHO = '/oauth/callback'
/** O caminho que o login do ChatGPT exige no retorno; o ouvinte atende os dois na mesma porta. */
export const CAMINHO_AUTH = '/auth/callback'
const CAMINHOS = [CAMINHO, CAMINHO_AUTH]
const ESPERA_MAX_MS = 10 * 60_000
const RESPOSTA_MAX_MS = 30_000
const CABECALHOS = {
  'Content-Type': 'text/html; charset=utf-8',
  'Cache-Control': 'no-store',
  'Referrer-Policy': 'no-referrer',
  'X-Content-Type-Options': 'nosniff',
  Connection: 'close',
  'Content-Security-Policy': "default-src 'none'; style-src 'unsafe-inline'",
}

const ERRO_PEDIDO = 'O pedido de login não é o que este app espera.' // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
const ERRO_OUTRO = 'Outro login começou.' // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
const ERRO_TEMPO = 'O login demorou demais.' // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
const ERRO_NAO_ABRIU = 'Não foi possível abrir o endereço de login.' // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra
const ERRO_CANCELADO = 'O login foi cancelado.' // textos-ok: erro técnico interno; o renderer só fecha o ouvinte e não o mostra

export const redirectDoApp = (porta: number, caminho: string = CAMINHO) => `http://127.0.0.1:${porta}${caminho}`

/** O login não voltou: outro começou no lugar, a pessoa desistiu ou o tempo acabou. */
export class LoginEncerrado extends Error {
  readonly motivo: 'cancelado' | 'tempo'
  constructor(motivo: 'cancelado' | 'tempo', message: string) {
    super(message)
    this.motivo = motivo
  }
}

const loopback = (u: URL) => u.hostname === '127.0.0.1' || u.hostname === 'localhost' || u.hostname === '[::1]'

/**
 * O pedido do Hub: a página de login do fornecedor (https; http só em loopback, como o Hub aceita) cujo endereço de
 * retorno é EXATAMENTE o do ouvinte deste app — `redirect_uri` (OAuth comum) ou `callback_url` (OpenRouter), um dos
 * dois, nunca os dois — e que traz um único `state`. O retorno pode pedir qualquer um dos dois caminhos conhecidos
 * do ouvinte; o navegador tem de voltar pelo mesmo. Qualquer outra coisa recusa: este ouvinte não abre login de
 * outro lugar nem espera um retorno que o Hub não tenha preparado para ele.
 */
export function lerPedido(url: unknown, porta: number): { state: string } | null {
  const pedido = lerPedidoTodo(url, porta)
  return pedido ? { state: pedido.state } : null
}

/** O pedido inteiro: o `state` e o caminho de retorno que ele pediu, para o navegador voltar pelo mesmo. */
function lerPedidoTodo(url: unknown, porta: number): { state: string; caminho: string } | null {
  if (typeof url !== 'string') return null
  let alvo: URL
  try { alvo = new URL(url) } catch { return null }
  if (alvo.username || alvo.password || alvo.hash) return null
  if (alvo.protocol !== 'https:' && !(alvo.protocol === 'http:' && loopback(alvo))) return null
  const retornos = [...alvo.searchParams.getAll('redirect_uri'), ...alvo.searchParams.getAll('callback_url')]
  const states = alvo.searchParams.getAll('state')
  const caminho = retornos.length === 1 ? CAMINHOS.find((c) => retornos[0] === redirectDoApp(porta, c)) : undefined
  if (!caminho) return null
  if (states.length !== 1 || states[0] === '') return null
  return { state: states[0], caminho }
}

const igual = (a: string, b: string) => {
  const x = Buffer.from(a)
  const y = Buffer.from(b)
  return x.length === y.length && timingSafeEqual(x, y)
}

const escapar = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c)

function pagina(idioma: AppLanguage, titulo: string, texto: string): string {
  const dir = idioma === 'ar-AE' ? 'rtl' : 'ltr'
  return `<!doctype html><html lang="${idioma}" dir="${dir}"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Mexerica</title>` +
    '<body style="font:16px system-ui;margin:20vh auto;max-width:28em;padding:0 16px;text-align:center">' +
    `<h2>${escapar(titulo)}</h2><p>${escapar(texto)}</p></body></html>`
}

export interface Resultado {
  ok: boolean
  /** O nome do conector, quando deu certo. */
  nome?: string
  /** A frase de recusa que o app já mostra na tela, quando não deu certo. */
  detalhe?: string
}

export interface Opcoes {
  porta: number
  abrir: (url: string) => Promise<boolean>
  idioma: () => AppLanguage
  esperaMs?: number
  respostaMs?: number
}

interface Espera {
  state: string
  /** O caminho que o pedido pediu no retorno; o navegador só volta por ele. */
  caminho: string
  resolve: (voltou: string) => void
  reject: (erro: Error) => void
  timer: NodeJS.Timeout
}

/** Um ouvinte de retorno por vez. `abrir` liga; `esperar` abre o navegador e aguarda; `responder` fecha a conversa com o navegador. */
export class RetornoDoLogin {
  private servidor: Server | null = null
  private portaReal = 0
  /** Cada `abrir` é uma tentativa nova; quem veio de uma tentativa antiga nunca mexe na atual. */
  private tentativa = 0
  private fila: Promise<void> = Promise.resolve()
  private espera: Espera | null = null
  private resposta: { res: ServerResponse; timer: NodeJS.Timeout } | null = null

  private readonly opcoes: Opcoes

  constructor(opcoes: Opcoes) {
    this.opcoes = opcoes
  }

  /** Liga o ouvinte em 127.0.0.1 e devolve a porta e o número da tentativa; `null` se não deu (porta ocupada): o login segue pelo caminho antigo. */
  abrir(): Promise<{ porta: number; tentativa: number } | null> {
    // Uma de cada vez: dois cliques seguidos não disputam a porta.
    const feito = this.fila.then(() => this.abrirAgora())
    this.fila = feito.then(() => undefined, () => undefined)
    return feito
  }

  private async abrirAgora(): Promise<{ porta: number; tentativa: number } | null> {
    this.fechar()
    const tentativa = ++this.tentativa
    const servidor = createServer((req, res) => this.atender(req, res))
    servidor.requestTimeout = 10_000
    servidor.headersTimeout = 10_000
    servidor.maxHeadersCount = 40
    const ligado = await new Promise<boolean>((fim) => {
      servidor.once('error', () => fim(false))
      servidor.listen(this.opcoes.porta, '127.0.0.1', () => fim(true))
    })
    // Outra tentativa começou enquanto esta ligava: esta não vale mais.
    if (!ligado || tentativa !== this.tentativa) { servidor.close(); return null }
    servidor.on('error', () => {})
    this.servidor = servidor
    this.portaReal = (servidor.address() as AddressInfo).port
    return { porta: this.portaReal, tentativa }
  }

  /** Abre a página de login e espera o retorno desta tentativa; devolve a URL de retorno (a mesma que o navegador chamou). */
  esperar(tentativa: unknown, url: unknown): Promise<string> {
    if (tentativa !== this.tentativa) return Promise.reject(new LoginEncerrado('cancelado', ERRO_OUTRO))
    const pedido = this.servidor ? lerPedidoTodo(url, this.portaReal) : null
    if (!pedido) { this.fechar(); return Promise.reject(new Error(ERRO_PEDIDO)) }
    const anterior = this.espera
    if (anterior) { clearTimeout(anterior.timer); anterior.reject(new LoginEncerrado('cancelado', ERRO_OUTRO)) }
    return new Promise<string>((resolve, reject) => {
      const espera: Espera = {
        state: pedido.state,
        caminho: pedido.caminho,
        resolve,
        reject,
        timer: setTimeout(() => {
          if (this.espera === espera) { this.espera = null; this.fechar() }
          reject(new LoginEncerrado('tempo', ERRO_TEMPO))
        }, this.opcoes.esperaMs ?? ESPERA_MAX_MS),
      }
      this.espera = espera
      void this.opcoes.abrir(String(url)).then((aberto) => {
        if (!aberto && this.espera === espera) {
          clearTimeout(espera.timer)
          this.espera = null
          this.fechar()
          reject(new Error(ERRO_NAO_ABRIU))
        }
      })
    })
  }

  /** O desfecho que o app deu ao retorno: vira a página que o navegador mostra, e o ouvinte fecha. */
  responder(tentativa: unknown, resultado: Resultado) {
    if (tentativa !== this.tentativa || !this.resposta) return
    const idioma = this.opcoes.idioma()
    const nome = typeof resultado.nome === 'string' ? resultado.nome.slice(0, 120) : ''
    const detalhe = typeof resultado.detalhe === 'string' ? resultado.detalhe.slice(0, 400) : ''
    this.fechar(resultado.ok === true
      ? pagina(idioma, traduzir(idioma, 'mercado.retorno.ok.titulo', { servico: nome }), traduzir(idioma, 'mercado.retorno.ok.texto'))
      : pagina(idioma, traduzir(idioma, 'mercado.retorno.erro.titulo'), detalhe || traduzir(idioma, 'mercado.retorno.erro.texto')))
  }

  /** O fim de UMA tentativa (o app desistiu ou falhou): só desliga se ela ainda é a atual. */
  encerrar(tentativa: unknown) {
    if (tentativa === this.tentativa) this.fechar()
  }

  /**
   * Desliga o ouvinte e cancela quem espera. Com um retorno já recebido, o navegador ainda ganha a página `html` (ou a
   * de "retorno recebido") antes de a conexão cair.
   */
  fechar(html?: string) {
    const espera = this.espera
    this.espera = null
    if (espera) { clearTimeout(espera.timer); espera.reject(new LoginEncerrado('cancelado', ERRO_CANCELADO)) }
    const servidor = this.servidor
    this.servidor = null
    servidor?.close()
    const pendente = this.resposta
    this.resposta = null
    if (!pendente) { servidor?.closeAllConnections(); return }
    clearTimeout(pendente.timer)
    const idioma = this.opcoes.idioma()
    pendente.res.writeHead(200, CABECALHOS)
    pendente.res.end(html ?? pagina(idioma, traduzir(idioma, 'mercado.retorno.recebido.titulo'), traduzir(idioma, 'mercado.retorno.recebido.texto')), () => servidor?.closeAllConnections())
  }

  private recusar(res: ServerResponse, status: number, titulo: string, texto: string) {
    res.writeHead(status, CABECALHOS)
    res.end(pagina(this.opcoes.idioma(), titulo, texto))
  }

  private atender(req: IncomingMessage, res: ServerResponse) {
    const idioma = this.opcoes.idioma()
    const parado = () => this.recusar(res, 404, traduzir(idioma, 'mercado.retorno.parado.titulo'), traduzir(idioma, 'mercado.retorno.parado.texto'))
    // Só o navegador deste computador, falando com o endereço exato do ouvinte (defende contra DNS rebinding).
    const host = req.headers.host
    if (req.method !== 'GET' || (host !== `127.0.0.1:${this.portaReal}` && host !== `localhost:${this.portaReal}`)) return parado()
    let alvo: URL
    try { alvo = new URL(req.url ?? '', `http://127.0.0.1:${this.portaReal}`) } catch { return parado() }
    const espera = this.espera
    // O retorno desta tentativa volta pelo caminho que o pedido pediu — um pedido em /oauth/callback não aceita
    // retorno em /auth/callback, nem o contrário — e tem exatamente um state, igual ao do pedido; o resto nunca passa.
    if (!espera || alvo.pathname !== espera.caminho) return parado()
    // O retorno desta tentativa tem exatamente um state, igual ao do pedido; o resto (outra aba, outro login) nunca passa.
    const states = alvo.searchParams.getAll('state')
    if (states.length !== 1 || !igual(states[0], espera.state)) return this.recusar(res, 400, traduzir(idioma, 'mercado.retorno.outro.titulo'), traduzir(idioma, 'mercado.retorno.outro.texto'))
    if (this.resposta) return this.recusar(res, 409, traduzir(idioma, 'mercado.retorno.recebido.titulo'), traduzir(idioma, 'mercado.retorno.recebido.texto'))
    // Sem desfecho do app em tempo útil, o navegador não fica pendurado.
    this.resposta = { res, timer: setTimeout(() => this.fechar(), this.opcoes.respostaMs ?? RESPOSTA_MAX_MS) }
    this.espera = null
    clearTimeout(espera.timer)
    espera.resolve(`http://127.0.0.1:${this.portaReal}${req.url ?? ''}`)
  }
}
