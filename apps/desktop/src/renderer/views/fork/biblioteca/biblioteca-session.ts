import { SpaceClient, type SpaceTransport } from '../../../../shared/fork-spaces-client.ts'
import {
  nativeAddBody, nativeFailure, nativeFailureText, parseNativeAdded, parseNativeCatalog,
  type NativeAddBody, type NativeAdded, type NativeCatalog,
} from '../../../../shared/fork-native-catalog.ts'
import { contaPadraoDoHub, contasDoMotor, escolhaDeConta } from '../../../../shared/fork-criar-bot-destino.ts'
import type { EngineAccounts, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { Observable } from '../../../lib/observable.ts'

// Biblioteca de especialistas (components/BibliotecaEspecialistas): o que a
// folha lê e pede ao Hub para UM space. Toda chamada parte de uma origem fixa: Hub (hello.hostId), link, época, space
// e geração local. Resposta de outra origem (outro space, queda, link novo, mesmo link depois de cair) é descartada
// antes de pintar. A prévia é efêmera, só desta sessão. Um Adicionar sem resposta guarda chave e corpo em memória
// (por Hub/space) e só é reenviado igual por Conferir resultado: nunca nasce outra chave sozinha.

/** O que a sessão lê da store do computador (BotStore); nunca escreve nela. */
export interface LibrarySource {
  readonly hello: { hostId: string } | null
  readonly connectedClient: SpaceTransport | null
  readonly principalEpoch: number
  /** Resposta do Hub a `donnaEngineAccess` (engine.accounts + canManage): sem ela, adicionar fica indisponível. */
  readonly canManageEngineAccounts: boolean
  subscribe(listener: () => void): () => void
}

export interface AddIntent { body: NativeAddBody }

export type LibraryPhase =
  | { kind: 'loading' }
  | { kind: 'ready' }
  | { kind: 'error'; message: string }
  /** O computador passou a ser outro Hub: nada do anterior fica à vista. */
  | { kind: 'moved' }

export type LibraryOp =
  | { kind: 'idle' }
  | { kind: 'sending'; intent: AddIntent }
  /** Sem resposta: não se sabe se adicionou. Só Conferir resultado (mesma intenção) ou Descartar. */
  | { kind: 'unknown'; intent: AddIntent }
  | { kind: 'refused'; catalogKey: string; message: string }
  /** Resposta definitiva do Hub para a intenção enviada (ativo ou recibo histórico removido). */
  | { kind: 'done'; added: NativeAdded }

interface Origin { source: LibrarySource; hostId: string; client: SpaceTransport; epoch: number; gen: number; space: SpaceClient }

/** Intenções sem resposta, só em memória deste app aberto, por Hub e space. Não é cache do catálogo. */
const pending = new Map<string, AddIntent>()
const pendingKey = (hostId: string, spaceId: string) => JSON.stringify([hostId, spaceId])

const READ_TIMEOUT = 20_000
const ACCOUNTS_TIMEOUT = 90_000

export interface LibraryAccounts { estado: 'carregando' | 'pronto' | 'falhou'; lista: ManagedAccount[] }

/** Fora do Pessoal, a conta como na criação manual num Space: a escolhida, se conectada, ou o padrão legado do Hub. */
export function contaParaAdicionar(contas: readonly ManagedAccount[], backend: string, podeGerenciar: boolean, escolhida: string | null): ManagedAccount | null {
  if (escolhaDeConta(podeGerenciar, backend) !== 'escolher') return null
  if (!escolhida) return contaPadraoDoHub(contas, backend)
  return contasDoMotor(contas, backend).find((c) => c.conta.id === escolhida && c.escolhivel)?.conta ?? null
}
const ADD_TIMEOUT = 60_000

export class LibrarySession extends Observable {
  phase: LibraryPhase = { kind: 'loading' }
  link: 'live' | 'offline' = 'live'
  catalog: NativeCatalog | null = null
  /** Hora da última prévia aceita, para “Última leitura às…”. */
  asOf: number | null = null
  op: LibraryOp = { kind: 'idle' }
  /** Fora do Pessoal: as contas de IA do Hub para a escolha, lidas na mesma origem da prévia. */
  accounts: LibraryAccounts = { estado: 'carregando', lista: [] }

  readonly spaceId: string
  readonly personal: boolean
  private origin: Origin | null = null
  private hostId: string | null = null
  private gen = 0
  private reads = 0
  private accountReads = 0
  private ops = 0
  private unwatch: (() => void) | null = null
  private source: LibrarySource | null = null
  private uuid: () => string

  constructor(spaceId: string, personal: boolean, uuid: () => string = () => crypto.randomUUID()) {
    super()
    this.spaceId = spaceId
    this.personal = personal
    this.uuid = uuid
  }

  /** Adicionar exige prévia lida neste link, autoridade de gestão do Hub e nenhuma intenção em curso ou sem resposta. */
  get canAdd() {
    return this.phase.kind === 'ready' && this.link === 'live' && !!this.catalog && !!this.source?.canManageEngineAccounts
      && this.op.kind !== 'sending' && this.op.kind !== 'unknown'
  }

  get authorized() { return !!this.source?.canManageEngineAccounts }

  open(source: LibrarySource) {
    this.release()
    this.source = source
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  /** Fechar não perde a intenção: em curso vira “sem resposta”, guardada para Conferir resultado ao reabrir. */
  close() {
    this.release()
    this.gen++
    this.ops++
    if (this.op.kind === 'sending') this.keepUnknown(this.op.intent)
    this.origin?.space.dispose()
    this.origin = null
    this.changed()
  }

  reread() {
    const origin = this.origin
    if (!origin || !this.current(origin)) return
    void this.read(origin)
    if (this.accounts.estado === 'falhou') void this.readAccounts(origin)
  }

  /** Lê as contas de novo se a última leitura falhou sem queda de conexão (a queda já relê ao voltar). */
  rereadAccounts() {
    const origin = this.origin
    if (this.accounts.estado === 'falhou' && origin && this.current(origin)) void this.readAccounts(origin)
  }

  /** Um gesto novo: chave nova, corpo a partir do vínculo da prévia atual. Só o Hub decide o efeito. */
  async add(catalogKey: string, backend: string | null, account: string | null = null): Promise<NativeAdded | null> {
    const origin = this.origin
    if (!this.canAdd || !origin || !this.catalog) return null
    let body: NativeAddBody
    try {
      body = nativeAddBody(this.catalog, catalogKey, { personal: this.personal, backend, account, key: this.uuid() })
    } catch (e) {
      this.op = { kind: 'refused', catalogKey, message: nativeFailureText(e) }
      this.changed()
      return null
    }
    return this.send(origin, { body })
  }

  /** Conferir resultado: reenvia exatamente a mesma intenção (chave e corpo). Nunca outra. */
  async retry(): Promise<NativeAdded | null> {
    const origin = this.origin
    if (this.op.kind !== 'unknown' || !origin || !this.current(origin) || this.link !== 'live' || !this.source?.canManageEngineAccounts) return null
    return this.send(origin, this.op.intent)
  }

  /** Esquece localmente a intenção sem resposta; não muda nada no Hub. Um novo gesto terá outra chave. */
  discard() {
    if (this.op.kind !== 'unknown' && this.op.kind !== 'refused' && this.op.kind !== 'done') return
    if (this.hostId) pending.delete(pendingKey(this.hostId, this.spaceId))
    this.op = { kind: 'idle' }
    this.changed()
    this.reread()
  }

  /**
   * R1-02: sai do recibo histórico Removido para o estado atual do space. É uma transição própria e explícita: o recibo
   * nunca tem destino além do bot dele, e só depois desta ação a folha volta a mostrar (e abrir) o vínculo de agora.
   */
  showCurrent() {
    if (this.op.kind !== 'done' || this.op.added.state !== 'removed') return
    this.op = { kind: 'idle' }
    this.changed()
    this.reread()
  }

  // MARK: origem

  private connect(source: LibrarySource) {
    this.origin?.space.dispose()
    const gen = ++this.gen
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    if (hostId && this.hostId && this.hostId !== hostId) this.forget()
    if (hostId) {
      this.hostId = hostId
      const saved = pending.get(pendingKey(hostId, this.spaceId))
      if (saved && this.op.kind === 'idle') this.op = { kind: 'unknown', intent: saved }
    }
    if (!hostId || !client) {
      this.origin = null
      this.link = 'offline'
      if (!this.catalog) this.phase = { kind: 'error', message: 'Sem conexão com o computador dos seus bots.' }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, gen, space: new SpaceClient(hostId, this.spaceId, client) }
    this.origin = origin
    this.link = 'live'
    if (!this.catalog) this.phase = { kind: 'loading' }
    this.changed()
    void this.read(origin)
    void this.readAccounts(origin)
  }

  /** A store mudou: outro Hub encerra; queda invalida a origem (mesmo que o link volte igual); link/época nova relê. */
  private watch(source: LibrarySource) {
    const hostId = source.hello?.hostId ?? null
    if (this.hostId && hostId && hostId !== this.hostId) {
      this.release()
      this.gen++
      this.ops++
      this.origin?.space.dispose()
      this.origin = null
      this.forget()
      this.phase = { kind: 'moved' }
      this.changed()
      return
    }
    const origin = this.origin
    if (!source.connectedClient) {
      if (origin || this.link === 'live') {
        this.gen++
        origin?.space.dispose()
        this.origin = null
        this.link = 'offline'
        this.changed()
      }
      return
    }
    if (!origin || origin.client !== source.connectedClient || origin.epoch !== source.principalEpoch || this.link === 'offline') this.connect(source)
    else this.changed()
  }

  private current(origin: Origin) {
    const { source } = origin
    return this.origin === origin && this.gen === origin.gen && source.hello?.hostId === origin.hostId
      && source.connectedClient === origin.client && source.principalEpoch === origin.epoch
  }

  private forget() {
    this.catalog = null
    this.asOf = null
    this.op = { kind: 'idle' }
    this.phase = { kind: 'loading' }
  }

  private release() {
    this.unwatch?.()
    this.unwatch = null
  }

  private keepUnknown(intent: AddIntent) {
    pending.set(pendingKey(intent.body.hubId, intent.body.spaceId), intent)
    this.op = { kind: 'unknown', intent }
  }

  // MARK: chamadas

  private async read(origin: Origin) {
    const read = ++this.reads
    try {
      const answer = await origin.space.call<unknown>('donnaNativeCatalog', {}, READ_TIMEOUT)
      if (!this.current(origin) || read !== this.reads) return
      this.catalog = parseNativeCatalog(answer, origin.hostId, this.spaceId)
      this.asOf = Date.now()
      this.phase = { kind: 'ready' }
    } catch (e) {
      if (!this.current(origin) || read !== this.reads) return
      const failure = nativeFailure(e)
      // Sem autoridade ou Hub sem biblioteca: nada antigo continua à vista.
      if (!this.catalog || failure === 'denied' || failure === 'old_hub' || failure === 'gone' || failure === 'moved') {
        this.catalog = null
        this.phase = { kind: 'error', message: nativeFailureText(e) }
      }
    }
    this.changed()
  }

  private async readAccounts(origin: Origin) {
    if (this.personal || !origin.source.canManageEngineAccounts) return
    const read = ++this.accountReads
    this.accounts = { estado: 'carregando', lista: [] }
    this.changed()
    try {
      const answer = await origin.client.call('donnaEngineAccounts', {}, ACCOUNTS_TIMEOUT) as EngineAccounts
      if (!this.current(origin) || read !== this.accountReads) return
      this.accounts = { estado: 'pronto', lista: answer.accounts }
    } catch {
      if (!this.current(origin) || read !== this.accountReads) return
      this.accounts = { estado: 'falhou', lista: [] }
    }
    this.changed()
  }

  private async send(origin: Origin, intent: AddIntent): Promise<NativeAdded | null> {
    const op = ++this.ops
    this.op = { kind: 'sending', intent }
    this.changed()
    try {
      const answer = await origin.space.call<unknown>('donnaNativeCatalogAdd', { ...intent.body }, ADD_TIMEOUT)
      if (op !== this.ops) return null
      // Origem trocada no caminho: o resultado é desconhecido para esta tela; a mesma intenção espera conferência.
      if (!this.current(origin)) return this.lost(intent)
      const added = parseNativeAdded(answer, intent.body)
      pending.delete(pendingKey(intent.body.hubId, intent.body.spaceId))
      this.op = { kind: 'done', added }
      this.changed()
      this.reread()
      return added
    } catch (e) {
      if (op !== this.ops) return null
      if (!this.current(origin)) return this.lost(intent)
      const failure = nativeFailure(e)
      if (failure === 'unknown') this.keepUnknown(intent)
      else {
        pending.delete(pendingKey(intent.body.hubId, intent.body.spaceId))
        this.op = { kind: 'refused', catalogKey: intent.body.catalogKey, message: nativeFailureText(e) }
        if (failure === 'conflict') this.reread()
      }
      this.changed()
      return null
    }
  }

  private lost(intent: AddIntent) {
    this.keepUnknown(intent)
    this.changed()
    return null
  }
}

/** Só para testes: esquece as intenções guardadas entre casos. */
export function resetLibraryPending() { pending.clear() }
