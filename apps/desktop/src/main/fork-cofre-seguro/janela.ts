import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import { BrowserWindow, ipcMain, type IpcMainInvokeEvent, type WebContents } from 'electron'
import { cofreCanais, ENTRY_LOGIN_NOVO, lerAbrir, lerAbrirNovo, lerEnvio, type AbrirCofre, type AvisoEstado, type DadosJanela, type EstadoJanela, type PedidoVisivel, type RespostaAbrir, type RespostaSalvar } from '../../shared/fork-cofre-seguro'
import { isGeneralContext, markPrivateContext } from '../fork-secret-box/contexts'
import { chamarHub, resolverHub, type CapturaHub, type FalhaHub, type FontesDeHub } from './hub'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { idiomaDoApp } from '../fork-idioma'

// MX-209 (fase 1 da MX-146): a janela do cartão seguro. Módulo próprio, sem relação com a caixa D277
// (`fork-secret-box`), que é outro contrato. O renderer manda SÓ a referência do pedido; o main resolve a
// conexão com o Hub, busca a entrada (`connectorRequestGet`, sem valor) e deriva dela todos os textos da
// janela. Caminho do valor: input da janela → IPC estreito do preload dela → este módulo → Hub
// (`connectorRequestFinish`) → cofre. Nunca passa pelo renderer do chat, pelo store, por log nem por
// broadcast. Não promete nada sobre o bot ver a tela: promete só que o valor não aparece na conversa.

const prefixoNonce = '--mx-cofre-nonce='
const METODO_GET = 'connectorRequestGet'
const METODO_FINISH = 'connectorRequestFinish'
// MX-277: guardar um login sem pedido do bot usa o caminho que o Hub já tem (o mesmo da tela Extensões → Credenciais).
const METODO_SALVAR_LOGIN = 'credentialSaveLogin'
// Leitura sem valor e sem cofre (`hello`), só para saber (antes de a pessoa digitar) que a captura do Hub está viva.
const METODO_ESTADO = 'hello'

interface Dependencias {
  /** A janela principal: a segura é filha modal dela. */
  pai: () => BrowserWindow | null
  /** As janelas gerais, para avisar "aberta/fechada/salva" (sem valor). */
  gerais: () => BrowserWindow[]
  hubs: FontesDeHub
  /** URL da página da janela segura (o preload dela tem caminho fixo, ao lado do geral). */
  url: string
  http?: typeof fetch
}

/** O que o Hub devolve em `connectorRequestGet`: a entrada, nunca o valor. */
interface PedidoHub {
  entryId: string
  botId: string
  botName: string
  kind: string
  status: string
  title: string
  site: string | null
  field: string | null
  location: string | null
  spaceId: string
  conversationId: string
}

interface Instancia {
  chave: string
  janela: BrowserWindow
  contents: WebContents
  nonce: string
  alvo: AbrirCofre
  /** A captura resolvida na abertura; invalidada se a conexão trocar ou a janela fechar. */
  captura: CapturaHub
  /** Os textos que a janela mostra — derivados da entrada no Hub, nunca do renderer. */
  pedido: PedidoVisivel
  /** Contexto canônico da entrada, para o finish; o renderer não escolhe. */
  contexto: { spaceId?: string; conversationId?: string }
  /** O bot da entrada, para a reconferência antes do envio. */
  botId: string
  vida: { viva: boolean }
  carregada: string | null
  salvando: boolean
  fechar: (estado?: EstadoJanela) => void
}

const abertas = new Map<string, Instancia>()
/** Aberturas em curso por chave: coalescem chamadas concorrentes do mesmo pedido antes do 1º await. */
const aberturas = new Map<string, Promise<RespostaAbrir>>()
let dependencias: Dependencias | null = null

const viva = (i: Instancia) => abertas.get(i.chave) === i && !i.janela.isDestroyed() && !i.contents.isDestroyed()
  && !isGeneralContext(i.contents) && i.carregada !== null && i.contents.getURL() === i.carregada
  && i.vida.viva

/** Ocupa a chave enquanto a janela existe e a instância vive — inclusive ainda carregando (antes do 1º paint). */
const ocupada = (i: Instancia) => abertas.get(i.chave) === i && !i.janela.isDestroyed() && i.vida.viva

/** Só a instância viva, o quadro principal dela e o nonce desta abertura valem. */
function daInstancia(event: IpcMainInvokeEvent, nonce: unknown): Instancia | null {
  for (const i of abertas.values()) {
    if (event.sender === i.contents && event.senderFrame === i.contents.mainFrame && viva(i) && typeof nonce === 'string' && nonce === i.nonce) return i
  }
  return null
}

function avisar(entryId: string, estado: EstadoJanela) {
  const aviso: AvisoEstado = { entryId, estado }
  for (const w of dependencias?.gerais() ?? []) if (!w.isDestroyed()) w.webContents.send(cofreCanais.estado, aviso)
}

const texto = (v: unknown, max: number): string | null =>
  typeof v === 'string' && v.trim() ? v.trim().slice(0, max) : null

/** Valida a resposta do Hub e deriva o pedido que a janela mostra; `null` = recusa. */
function pedidoDoHub(bruto: unknown, alvo: AbrirCofre): { pedido: PedidoVisivel; contexto: { spaceId?: string; conversationId?: string } } | null {
  if (!bruto || typeof bruto !== 'object') return null
  const h = bruto as Record<string, unknown>
  // `code` (MX-286): o código de login usa a mesma janela; a entrada nunca leva o código.
  if (h.entryId !== alvo.entryId || (h.kind !== 'login' && h.kind !== 'secret' && h.kind !== 'code') || h.status !== 'pending') return null
  // O contexto que o chat apontou tem que ser o da entrada: espaço e conversa divergentes = outro pedido.
  if ((alvo.spaceId && texto(h.spaceId, 128) !== alvo.spaceId) || (alvo.conversationId && texto(h.conversationId, 128) !== alvo.conversationId)) return null
  const titulo = texto(h.title, 200), botNome = texto(h.botName, 100), botId = texto(h.botId, 128)
  if (!titulo || !botNome || !botId) return null
  const login = h.kind === 'login' || h.kind === 'code'
  const spaceId = texto(h.spaceId, 128), conversationId = texto(h.conversationId, 128)
  return {
    pedido: {
      tipo: h.kind, titulo, botNome,
      site: login ? texto(h.site, 200) ?? titulo : null,
      campo: login ? null : texto(h.field, 128),
      local: login ? null : texto(h.location, 16),
    },
    contexto: { ...(spaceId ? { spaceId } : {}), ...(conversationId ? { conversationId } : {}) },
  }
}

/** Os campos que o consentimento mostra precisam ser os mesmos na reconferência. */
function mesmoPedido(h: PedidoHub, pedido: PedidoVisivel): boolean {
  const login = pedido.tipo === 'login' || pedido.tipo === 'code'
  return h.kind === pedido.tipo
    && h.status === 'pending'
    && texto(h.title, 200) === pedido.titulo
    && texto(h.botName, 100) === pedido.botNome
    && (login
      ? (texto(h.site, 200) ?? texto(h.title, 200)) === pedido.site
      : texto(h.field, 128) === pedido.campo && texto(h.location, 16) === pedido.local)
}

async function lerPedido(captura: CapturaHub, alvo: AbrirCofre, http: typeof fetch): Promise<{ pedido: PedidoVisivel; contexto: { spaceId?: string; conversationId?: string }; cru: PedidoHub }> {
  const bruto = await chamarHub(captura, METODO_GET, {
    entryId: alvo.entryId,
    ...(alvo.spaceId ? { spaceId: alvo.spaceId } : {}),
    ...(alvo.conversationId ? { conversationId: alvo.conversationId } : {}),
    ...(alvo.hubId ? { hubId: alvo.hubId } : {}),
  }, http, 20_000)
  const lido = pedidoDoHub(bruto, alvo)
  if (!lido) throw { status: 400 } satisfies FalhaHub
  return { ...lido, cru: bruto as PedidoHub }
}

/** Chat → main. Só ids entram; o main busca a entrada e deriva os textos. Só um aviso sem valor volta. */
function aoAbrir(event: IpcMainInvokeEvent, bruto: unknown): Promise<RespostaAbrir> {
  const dep = dependencias
  if (!dep || !isGeneralContext(event.sender) || event.senderFrame !== event.sender.mainFrame) return Promise.resolve({ ok: false, motivo: 'invalido' })
  const alvo = lerAbrir(bruto)
  // O "pedido" do login novo não existe no Hub: este canal nunca o abre (o canal próprio abaixo abre, sem valor).
  if (!alvo || alvo.entryId === ENTRY_LOGIN_NOVO) return Promise.resolve({ ok: false, motivo: 'invalido' })
  const chave = `${alvo.computerId}/${alvo.entryId}`
  const existente = abertas.get(chave)
  if (existente && ocupada(existente)) {
    // Uma janela por pedido, inclusive durante o carregamento: reabrir não cria nem destrói nada.
    if (viva(existente)) {
      if (existente.janela.isMinimized()) existente.janela.restore()
      existente.janela.focus()
      return Promise.resolve({ ok: true, foco: true })
    }
    return Promise.resolve({ ok: true, foco: false })
  }
  // Reserva ANTES do primeiro await: duas chamadas concorrentes do mesmo pedido dividem a mesma abertura.
  const corrente = aberturas.get(chave)
  if (corrente) return corrente
  const tentativa = abrirAoVivo(dep, chave, alvo)
  aberturas.set(chave, tentativa)
  return tentativa.finally(() => {
    if (aberturas.get(chave) === tentativa) aberturas.delete(chave)
  })
}

/**
 * MX-277: chat → main, "abra a janela segura VAZIA para guardar um login novo". Só o id do computador entra; o trecho
 * escondido do aviso de senha colada NÃO vem junto (o formato recusa qualquer outro campo) — a pessoa cola de novo,
 * direto na janela. Mesmas guardas da janela do pedido: remetente geral no quadro principal, Hub local/SSH capturado
 * com posse reconferida (relay recusado), uma janela por abertura.
 */
function aoAbrirNovo(event: IpcMainInvokeEvent, bruto: unknown): Promise<RespostaAbrir> {
  const dep = dependencias
  if (!dep || !isGeneralContext(event.sender) || event.senderFrame !== event.sender.mainFrame) return Promise.resolve({ ok: false, motivo: 'invalido' })
  const pedido = lerAbrirNovo(bruto)
  if (!pedido) return Promise.resolve({ ok: false, motivo: 'invalido' })
  const alvo: AbrirCofre = { computerId: pedido.computerId, entryId: ENTRY_LOGIN_NOVO }
  const chave = `${alvo.computerId}/${alvo.entryId}`
  const existente = abertas.get(chave)
  if (existente && ocupada(existente)) {
    if (viva(existente)) {
      if (existente.janela.isMinimized()) existente.janela.restore()
      existente.janela.focus()
      return Promise.resolve({ ok: true, foco: true })
    }
    return Promise.resolve({ ok: true, foco: false })
  }
  const corrente = aberturas.get(chave)
  if (corrente) return corrente
  const tentativa = abrirNovoAoVivo(dep, chave, alvo)
  aberturas.set(chave, tentativa)
  return tentativa.finally(() => {
    if (aberturas.get(chave) === tentativa) aberturas.delete(chave)
  })
}

async function abrirNovoAoVivo(dep: Dependencias, chave: string, alvo: AbrirCofre): Promise<RespostaAbrir> {
  abertas.get(chave)?.fechar()
  const vida = { viva: true }
  const captura = resolverHub(alvo.computerId, dep.hubs, () => vida.viva)
  if (!captura) return { ok: false, motivo: 'semSuporte' }
  try {
    // Sem valor e sem pedido: só confere que esta captura chega ao Hub antes de a pessoa digitar.
    await chamarHub(captura, METODO_ESTADO, {}, dep.http ?? fetch, 20_000)
  } catch (erro) {
    vida.viva = false
    return { ok: false, motivo: (erro as FalhaHub).status === 0 || (erro as FalhaHub).status >= 500 ? 'semConexao' : 'invalido' }
  }
  const ocupante = abertas.get(chave)
  if (ocupante && ocupada(ocupante)) return { ok: true, foco: viva(ocupante) }
  const pedido: PedidoVisivel = { tipo: 'loginNovo', titulo: '', site: null, campo: null, local: null, botNome: '' }
  criar(dep, chave, alvo, captura, vida, pedido, {}, '')
  return { ok: true, foco: false }
}

async function abrirAoVivo(dep: Dependencias, chave: string, alvo: AbrirCofre): Promise<RespostaAbrir> {
  abertas.get(chave)?.fechar()
  // A conexão é capturada aqui e fica presa a esta abertura; se o túnel cair ou trocar, ela morre.
  const vida = { viva: true }
  const captura = resolverHub(alvo.computerId, dep.hubs, () => vida.viva)
  if (!captura) return { ok: false, motivo: 'semSuporte' }
  let lido: Awaited<ReturnType<typeof lerPedido>>
  try {
    lido = await lerPedido(captura, alvo, dep.http ?? fetch)
  } catch (erro) {
    vida.viva = false
    const status = (erro as FalhaHub).status
    return { ok: false, motivo: status === 0 || status >= 500 ? 'semConexao' : 'invalido' }
  }
  // Outra abertura pode ter concluído primeiro (ou a janela pode ter sido fechada durante o GET).
  const ocupante = abertas.get(chave)
  if (ocupante && ocupada(ocupante)) return { ok: true, foco: viva(ocupante) }
  criar(dep, chave, alvo, captura, vida, lido.pedido, lido.contexto, lido.cru.botId)
  return { ok: true, foco: false }
}

function criar(dep: Dependencias, chave: string, alvo: AbrirCofre, captura: CapturaHub, vida: { viva: boolean }, pedido: PedidoVisivel, contexto: { spaceId?: string; conversationId?: string }, botId: string) {
  const nonce = randomUUID()
  const pai = dep.pai()
  const janela = new BrowserWindow({
    width: 460, height: 640, minWidth: 420, minHeight: 520, show: false,
    title: traduzir(idiomaDoApp(), 'extensoes.cofre.janelaTitulo'),
    ...(pai && !pai.isDestroyed() ? { parent: pai, modal: true } : {}),
    webPreferences: {
      preload: join(__dirname, '../preload/fork-cofre-seguro.js'), additionalArguments: [`${prefixoNonce}${nonce}`],
      sandbox: true, contextIsolation: true, nodeIntegration: false, devTools: false, spellcheck: false,
    },
  })
  let encerrada = false
  const instancia: Instancia = {
    chave, janela, contents: janela.webContents, nonce, alvo, captura, pedido, contexto, botId, vida, carregada: null, salvando: false,
    fechar: (estado = 'fechada') => {
      if (encerrada) return
      encerrada = true
      vida.viva = false
      if (abertas.get(chave) === instancia) abertas.delete(chave)
      if (!janela.isDestroyed()) janela.destroy()
      avisar(alvo.entryId, estado)
    },
  }
  abertas.set(chave, instancia)
  try {
    // Marcada como privada ANTES de carregar: o host-proxy e os broadcasts nunca a tratam como janela geral.
    markPrivateContext(janela.webContents)
    janela.setContentProtection(true)
    janela.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    janela.webContents.on('will-navigate', (e) => e.preventDefault())
    janela.webContents.on('will-redirect', (e) => e.preventDefault())
    janela.webContents.on('render-process-gone', () => instancia.fechar())
    janela.on('closed', () => instancia.fechar())
    void janela.loadURL(dep.url).then(() => {
      instancia.carregada = janela.webContents.getURL()
      // Depois de carregada, qualquer navegação nova (inclusive recarga) encerra esta instância.
      janela.webContents.on('did-start-navigation', (_e, _url, mesmaPagina, principal) => { if (principal && !mesmaPagina) instancia.fechar() })
      janela.show()
      avisar(alvo.entryId, 'aberta')
    }).catch(() => instancia.fechar())
  } catch {
    instancia.fechar()
  }
}

function aoPedirDados(event: IpcMainInvokeEvent, nonce: unknown): DadosJanela | null {
  const i = daInstancia(event, nonce)
  return i ? { pedido: i.pedido } : null
}

async function aoSalvar(event: IpcMainInvokeEvent, nonce: unknown, bruto: unknown): Promise<RespostaSalvar> {
  const dep = dependencias
  const i = daInstancia(event, nonce)
  if (!dep || !i) return { ok: false, erro: 'fechada' }
  const envio = lerEnvio(bruto, i.pedido.tipo)
  if (!envio) return { ok: false, erro: 'vazio' }
  if (i.salvando) return { ok: false, erro: 'fechada' }
  if (i.pedido.tipo === 'loginNovo') return salvarLoginNovo(dep, i, envio)
  // Reconferir a MESMA entrada e a MESMA captura imediatamente antes de enviar o valor:
  // o pedido precisa continuar pendente e com os mesmos textos que a janela mostrou.
  let deNovo: Awaited<ReturnType<typeof lerPedido>>
  try {
    deNovo = await lerPedido(i.captura, i.alvo, dep.http ?? fetch)
  } catch (erro) {
    const status = (erro as FalhaHub).status
    return { ok: false, erro: status === 0 || status >= 500 ? 'conexao' : 'recusado' }
  }
  if (!mesmoPedido(deNovo.cru, i.pedido) || deNovo.cru.botId !== i.botId) {
    i.fechar()
    return { ok: false, erro: 'recusado' }
  }
  i.salvando = true
  try {
    await chamarHub(i.captura, METODO_FINISH, {
      entryId: i.alvo.entryId, connectorId: null,
      value: envio.valor, username: i.pedido.tipo === 'login' ? envio.usuario : null,
      label: i.pedido.tipo === 'login' ? envio.rotulo : null, loginId: null,
      ...(deNovo.contexto.spaceId ? { spaceId: deNovo.contexto.spaceId } : {}),
      ...(i.alvo.hubId ? { hubId: i.alvo.hubId } : {}),
      ...(deNovo.contexto.conversationId ? { conversationId: deNovo.contexto.conversationId } : {}),
    }, dep.http ?? fetch)
  } catch (erro) {
    i.salvando = false
    // Só o status decide a mensagem; o corpo e o erro do Hub nunca seguem adiante.
    return { ok: false, erro: (erro as FalhaHub).status === 0 ? 'conexao' : 'recusado' }
  }
  // O cartão da conversa muda pela atualização normal da entrada no Hub (status ready); aqui só se fecha.
  setImmediate(() => i.fechar('salva'))
  return { ok: true }
}

/**
 * MX-277: guarda o login novo pelo caminho que o Hub já tem (`credentialSaveLogin`), chamado AQUI no main com a captura
 * da abertura. Não há pedido do bot a reconferir; valem as guardas do `chamarHub` (posse e validade antes e depois,
 * `redirect: 'error'`, só o status volta). `createOnly`: nunca substitui um login existente (o Hub recusa com 409). O valor existe só na fila do fetch e nunca vai ao chat, ao log nem a broadcast.
 */
async function salvarLoginNovo(dep: Dependencias, i: Instancia, envio: NonNullable<ReturnType<typeof lerEnvio>>): Promise<RespostaSalvar> {
  i.salvando = true
  try {
    await chamarHub(i.captura, METODO_SALVAR_LOGIN, {
      site: envio.site ?? '', username: envio.usuario, password: envio.valor, label: envio.rotulo,
      // Criação exclusiva, conferida e gravada pelo Hub sob a mesma exclusão: nunca substitui um login existente.
      createOnly: true,
    }, dep.http ?? fetch)
  } catch (erro) {
    i.salvando = false
    const status = (erro as FalhaHub).status
    // 409: já existe login para este site e usuário (nada foi alterado). Só o status decide; o corpo nunca volta.
    return { ok: false, erro: status === 0 ? 'conexao' : status === 409 ? 'jaExiste' : 'recusado' }
  }
  // O chat só recebe "salva" (sem valor); a janela se fecha.
  setImmediate(() => i.fechar('salva'))
  return { ok: true }
}

function aoCancelar(event: IpcMainInvokeEvent, nonce: unknown) {
  daInstancia(event, nonce)?.fechar()
}

export function registrarCofreSeguro(dep: Dependencias) {
  dependencias = dep
  ipcMain.handle(cofreCanais.abrir, aoAbrir)
  ipcMain.handle(cofreCanais.abrirNovo, aoAbrirNovo)
  ipcMain.handle(cofreCanais.dados, aoPedirDados)
  ipcMain.handle(cofreCanais.salvar, aoSalvar)
  ipcMain.on(cofreCanais.cancelar, aoCancelar)
}
