import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { SpacesSnapshot, SpaceConversation } from '../../../../shared/fork-spaces.ts'
import type { Entry } from '../../../../shared/models.ts'
import { botName as nomeDoBot, continuityText as textoDaContinuidade, conversationGroups, conversationLabel as rotuloDaConversa, mergeHistory, readErrorText, retainedSpace } from '../../../lib/fork-space-context.ts'
import { cru, textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// D454 (fatia 9c) e D507: os helpers devolvem `Frase`; aqui o texto pt-BR dela, sem as marcas de isolamento.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : f ?? null)
const botName = (...a: Parameters<typeof nomeDoBot>) => pt(nomeDoBot(...a))
const continuityText = (...a: Parameters<typeof textoDaContinuidade>) => pt(textoDaContinuidade(...a))
const conversationLabel = (c: SpaceConversation, nome: string) => pt(rotuloDaConversa(c, cru(nome)))
import { newest, openSpaceAt, SpaceContext, type SpaceLink, type SpaceSource } from '../../../store/fork-space-context.ts'

// D169 (SpaceView): abrir um space lê o Hub pelo SpaceClient/ConversationClient e só pinta o que é daquele Hub, space e
// conversa; resposta atrasada, outro Hub no mesmo computador ou evento de outro contexto nunca chega à tela. Só leitura.

const settle = () => new Promise((r) => setTimeout(r, 0))
const conversation = (id: string, spaceId: string, extra: Partial<SpaceConversation> = {}): SpaceConversation => ({
  id, spaceId, botId: 'donna', title: id, revision: 1, stateRevision: 1, primary: false, closed: false, readRevision: 0,
  engineAccountId: null, engineSelection: null, accountRevision: null, continuity: null, ...extra,
})
const entry = (id: string, seq: number, spaceId: string, conversationId: string, kind = 'user', extra: Partial<Entry['data']> = {}): Entry => ({
  id, seq, botId: conversationId, spaceId, conversationId, rev: seq, kind, turn: 0, data: { text: id, ...(kind === 'agent' ? { final: true } : {}), ...extra }, createdAt: 0, updatedAt: 0,
})
const deferred = <T>() => { let resolve!: (v: T) => void; const promise = new Promise<T>((r) => { resolve = r }); return { promise, resolve } }

/** A Hub link that answers by hand: every call and stream is recorded, nothing reaches a network. */
class FakeLink implements SpaceLink {
  calls: { method: string; body: Record<string, unknown> }[] = []
  streams: { spaceId: string; since: number; onEvent: (e: never) => void; onEnd: (error: unknown) => void; stopped: boolean }[] = []
  answer: (method: string, body: Record<string, unknown>) => Promise<unknown>
  readonly hostId: string
  constructor(hostId: string, answer?: FakeLink['answer']) {
    this.hostId = hostId
    this.answer = answer ?? ((method, body) => Promise.resolve(standard(this.hostId, method, body)))
  }
  call(method: string, body: unknown) {
    this.calls.push({ method, body: body as Record<string, unknown> })
    return this.answer(method, body as Record<string, unknown>)
  }
  inSpace(spaceId: string) {
    return { events: (since: number, _client: string, onEvent: (e: never) => void, onEnd: (error: unknown) => void) => {
      const s = { spaceId, since, onEvent, onEnd, stopped: false }
      this.streams.push(s)
      return () => { s.stopped = true }
    } }
  }
  emit(e: unknown) { for (const s of this.streams) if (!s.stopped) s.onEvent(e as never) }
}

function standard(hubId: string, method: string, body: Record<string, unknown>) {
  const spaceId = body.spaceId as string
  if (method === 'donnaConversations') return { hubId, spaceId, conversations: [conversation(`${spaceId}-fixa`, spaceId, { primary: true }), conversation(`${spaceId}-c2`, spaceId, { title: 'Planejamento' })], principal: { id: 'donna', name: 'Donna' } }
  if (method === 'donnaConversationHistory') {
    const id = body.conversationId as string
    return { hubId, context: { version: 1, spaceId, conversationId: id, botId: 'donna', conversationRevision: 1 }, entries: [entry(`${id}-b`, 2, spaceId, id, 'agent'), entry(`${id}-tool`, 3, spaceId, id, 'tool'), entry(`${id}-a`, 1, spaceId, id)], hasMore: false }
  }
  throw new Error('unknown_method')
}

class FakeSource implements SpaceSource {
  hello: { hostId: string; rev: number; spacesContract?: number } | null
  connectedClient: SpaceLink | null
  principalEpoch = 1
  spaces: SpacesSnapshot | null = null
  listeners = new Set<() => void>()
  constructor(link: FakeLink, contract: number | null = 1) {
    this.hello = { hostId: link.hostId, rev: 40, spacesContract: contract ?? undefined }
    this.connectedClient = link
  }
  subscribe(l: () => void) { this.listeners.add(l); return () => void this.listeners.delete(l) }
  changed() { for (const l of [...this.listeners]) l() }
}

test('abrir lê as conversas e o histórico daquele space, com Hub e space injetados, só leitura', async () => {
  const link = new FakeLink('hub-a')
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 's1')
  await settle()
  assert.equal(ctx.list.state, 'ready')
  assert.deepEqual(ctx.list.conversations.map((c) => c.id), ['s1-fixa', 's1-c2'])
  assert.equal(ctx.conversationId, 's1-fixa', 'abre a primeira conversa aberta, na ordem do Hub')
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['s1-fixa-a', 's1-fixa-b'], 'só linhas de conversa, em ordem de seq')
  for (const c of link.calls) assert.deepEqual([c.body.hubId, c.body.spaceId], ['hub-a', 's1'])
  assert.deepEqual(link.calls.map((c) => c.method), ['donnaConversations', 'donnaConversationHistory'], 'nada envia, enfileira, para ou marca lida')
  // R2: desde 0, para o catch-up do Hub trazer os bots que o space já tinha antes do hello (e seus nomes).
  assert.deepEqual([link.streams[0]?.spaceId, link.streams[0]?.since], ['s1', 0], 'stream do próprio space, catch-up completo')
})

test('troca de space e de conversa: resposta atrasada (inclusive A→B→A) é descartada antes de pintar', async () => {
  const pending = new Map<string, ReturnType<typeof deferred<unknown>>>()
  const link = new FakeLink('hub-a', (method, body) => {
    const key = `${method}:${body.spaceId}:${body.conversationId ?? ''}:${pending.size}`
    const d = deferred<unknown>()
    pending.set(key, d)
    d.promise.then(() => {})
    return d.promise.then(() => standard('hub-a', method, body))
  })
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  ctx.open(source, 'A')
  ctx.open(source, 'B')
  ctx.open(source, 'A')
  const [firstA, onlyB, secondA] = [...pending.values()]
  onlyB!.resolve(undefined)
  firstA!.resolve(undefined)
  await settle()
  assert.equal(ctx.list.state, 'loading', 'as respostas da primeira A e de B não pintam a A aberta agora')
  secondA!.resolve(undefined)
  await settle()
  assert.deepEqual(ctx.list.conversations.map((c) => c.spaceId), ['A', 'A'])
  for (const d of pending.values()) d.resolve(undefined)
  await settle()
  ctx.select('A-c2')
  ctx.select('A-fixa')
  for (const d of pending.values()) d.resolve(undefined)
  await settle()
  assert.equal(ctx.conversationId, 'A-fixa')
  assert.ok(ctx.history.entries.every((e) => e.conversationId === 'A-fixa'), 'o histórico da conversa trocada não aparece na atual')
})

test('outro Hub no mesmo store fecha a vista; conexão caída guarda o último dado; link novo relê', async () => {
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  ctx.open(source, 's1')
  await settle()
  source.connectedClient = null
  source.changed()
  assert.equal(ctx.link, 'offline')
  assert.equal(ctx.list.conversations.length, 2, 'sem conexão, a lista fica com o último estado')
  assert.ok(ctx.asOf)
  const link2 = new FakeLink('hub-a')
  source.connectedClient = link2
  source.principalEpoch = 2
  source.changed()
  await settle()
  assert.equal(ctx.link, 'live')
  assert.deepEqual(link2.calls.map((c) => c.method), ['donnaConversations', 'donnaConversationHistory'], 'relê pelo link novo, só leitura')
  source.hello = { hostId: 'hub-b', rev: 1, spacesContract: 1 }
  source.changed()
  assert.equal(ctx.spaceId, null)
  assert.equal(ctx.list.conversations.length, 0, 'nada do Hub anterior fica na tela')
  assert.equal(ctx.history.entries.length, 0)
})

test('Hub sem Spaces, recusa e lista vazia nunca viram a mesma coisa', async () => {
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(new FakeLink('hub-a'), null), 's1')
  assert.equal(ctx.list.state, 'error')
  assert.equal(pt(ctx.list.error?.text), 'Conversas ainda indisponíveis neste Hub.')
  const refused = new FakeLink('hub-a', () => Promise.reject(new Error('space_not_found')))
  ctx.open(new FakeSource(refused), 's1')
  await settle()
  assert.equal(ctx.list.error?.kind, 'gone')
  assert.equal(readErrorText(Object.assign(new Error('x'), { status: 0 })).kind, 'offline')
  assert.ok(!/_/.test(pt(readErrorText(new Error('algo_novo')).text)!), 'nunca o código cru')
})

test('eventos: só os do space e da conversa abertos; o que chega durante a leitura entra na página', async () => {
  const history = deferred<void>()
  const link = new FakeLink('hub-a', async (method, body) => {
    if (method === 'donnaConversationHistory') await history.promise
    return standard('hub-a', method, body)
  })
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 's1')
  await settle()
  link.emit({ type: 'entry', spaceId: 's1', entry: entry('ao-vivo', 9, 's1', 's1-fixa', 'agent') })
  link.emit({ type: 'entry', spaceId: 's1', entry: entry('outra', 10, 's1', 's1-c2') })
  link.emit({ type: 'entry', spaceId: 's2', entry: entry('alheia', 11, 's2', 's1-fixa') })
  history.resolve()
  await settle()
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['s1-fixa-a', 's1-fixa-b', 'ao-vivo'])
  link.emit({ type: 'conversation', spaceId: 's1', conversation: conversation('s1-c2', 's1', { title: 'Novo título', stateRevision: 5 }) })
  link.emit({ type: 'conversation', spaceId: 's2', conversation: conversation('s2-x', 's2') })
  assert.deepEqual(ctx.list.conversations.map((c) => [c.id, c.title]), [['s1-fixa', 's1-fixa'], ['s1-c2', 'Novo título']], 'atualização mantém a ordem do Hub')
  link.streams[0]!.onEnd(null)
  assert.equal(ctx.link, 'paused', 'fim limpo também não pode anunciar atualizações ao vivo')
  ctx.retry()
  await settle()
  assert.equal(ctx.link, 'live')
  link.streams.at(-1)!.onEnd(new Error('stream'))
  assert.equal(ctx.link, 'paused')
})

test('mensagens anteriores começam abaixo da mais antiga que o Hub mandou, sem buraco', async () => {
  const link = new FakeLink('hub-a', (method, body) => {
    if (method !== 'donnaConversationHistory') return Promise.resolve(standard('hub-a', method, body))
    const id = body.conversationId as string
    const older = body.beforeSeq !== undefined
    return Promise.resolve({ hubId: 'hub-a', entries: older ? [entry('velha', 5, 's1', id)] : [entry(`${id}-tool`, 50, 's1', id, 'tool'), entry('nova', 51, 's1', id)], hasMore: !older })
  })
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 's1')
  await settle()
  link.emit({ type: 'entry', spaceId: 's1', entry: entry('do-stream', 20, 's1', 's1-fixa') })
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['nova'], 'entrada mais velha que a página espera as anteriores')
  ctx.loadOlder()
  await settle()
  assert.equal(link.calls.at(-1)?.body.beforeSeq, 50, 'abaixo da mais antiga da página, mesmo a que não aparece')
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['velha', 'nova'])
  assert.equal(ctx.history.hasMore, false)
})

test('regras puras: grupos, rótulos, nomes, continuidade e mescla por versão', () => {
  const fixa = conversation('f', 's', { primary: true })
  const fechada = conversation('x', 's', { closed: true })
  assert.deepEqual(conversationGroups([fixa, fechada]), { open: [fixa], closed: [fechada] })
  assert.equal(conversationLabel(fixa, 'Donna'), 'Conversa fixa com Donna')
  assert.equal(botName('b9', { id: 'donna', name: 'Donna' }, new Map()), 'Bot deste space')
  assert.equal(continuityText(conversation('c', 's', { continuity: { mode: 'fresh_reconstructed', evidence: 'prepared_by_hub', coverage: { kind: 'bounded_visible_messages', includesAttachments: false, includesTools: false, includesApprovals: false, messages: 12, maxMessages: 40, scanLimit: 200, maxCharsPerMessage: 8192, maxResponseBytes: 65536, truncated: false, contextStartSeq: 3 } } })),
    'Começou com as últimas 12 mensagens visíveis desta conversa, sem anexos, ferramentas e aprovações.')
  const older = entry('e', 1, 's', 'c')
  assert.equal(mergeHistory([{ ...older, rev: 5, data: { text: 'novo' } }], [{ ...older, rev: 2, data: { text: 'velho' } }])[0]!.data.text, 'novo', 'versão mais velha não volta')
  assert.deepEqual(newest([fixa], [conversation('n', 's')], false).map((c) => c.id), ['f', 'n'])
  assert.deepEqual(newest([fixa, fechada], [fechada, fixa], true).map((c) => c.id), ['x', 'f'], 'a lista inteira do Hub manda na ordem')
})

// PR96 Opus1 F1: every history read belongs to one selection, including its error and live buffer.
test('A→B→A descarta a leitura anterior antes e depois da nova, conservando a entrada ao vivo', async () => {
  for (const oldFirst of [false, true]) {
    const pending: { body: Record<string, unknown>; resolve: (v: unknown) => void }[] = []
    const link = new FakeLink('hub-a', (method, body) => method === 'donnaConversations'
      ? Promise.resolve(standard('hub-a', method, body))
      : new Promise((resolve) => pending.push({ body, resolve })))
    const ctx = new SpaceContext()
    ctx.open(new FakeSource(link), 's1')
    await settle()
    ctx.select('s1-c2')
    ctx.select('s1-fixa')
    const answer = (n: number) => pending[n]!.resolve(standard('hub-a', 'donnaConversationHistory', pending[n]!.body))
    if (oldFirst) {
      answer(0)
      await settle()
      assert.equal(ctx.history.state, 'loading', 'erro da primeira seleção não encerra a leitura atual')
    }
    link.emit({ type: 'entry', spaceId: 's1', entry: entry('ao-vivo', 4, 's1', 's1-fixa') })
    answer(2)
    await settle()
    if (!oldFirst) answer(0)
    answer(1)
    await settle()
    assert.equal(ctx.history.state, 'ready')
    assert.equal(ctx.history.error, null)
    assert.deepEqual(ctx.history.entries.map((e) => e.id), ['s1-fixa-a', 's1-fixa-b', 'ao-vivo'])
    ctx.close()
  }
})

test('anteriores de uma seleção descartada não mostram um vazio falso na nova seleção da mesma conversa', async () => {
  const pending: { body: Record<string, unknown>; resolve: (v: unknown) => void }[] = []
  const link = new FakeLink('hub-a', (method, body) => method === 'donnaConversations'
    ? Promise.resolve(standard('hub-a', method, body))
    : new Promise((resolve) => pending.push({ body, resolve })))
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 's1')
  await settle()
  pending[0]!.resolve({ ...standard('hub-a', 'donnaConversationHistory', pending[0]!.body), hasMore: true })
  await settle()
  ctx.loadOlder()
  ctx.select('s1-c2')
  ctx.select('s1-fixa')
  pending[1]!.resolve(standard('hub-a', 'donnaConversationHistory', pending[1]!.body))
  await settle()
  assert.equal(ctx.history.state, 'loading')
  assert.equal(ctx.history.error, null)
  pending[3]!.resolve(standard('hub-a', 'donnaConversationHistory', pending[3]!.body))
  pending[2]!.resolve(standard('hub-a', 'donnaConversationHistory', pending[2]!.body))
  await settle()
  assert.equal(ctx.history.state, 'ready')
  assert.equal(ctx.history.entries.length, 2)
  ctx.close()
})

test('a origem aberta continua na faixa offline, mas não sobrevive a outro Hub, ocultação ou remoção confirmada', async () => {
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const item: SpacesSnapshot['spaces'][number] = {
    id: 's1', kind: 'space', name: 'Estúdio', position: 1, revision: 1, tile: null, owner: null,
    appearanceConfigured: true, appearance: { wallpaper: 'none', blur: 0, transparency: 50, veilLight: 0.18, veilDark: 0.32 },
  }
  source.spaces = { version: 1, revision: 1, personalSpaceId: 'pessoal', spaces: [item] }
  const origin = { store: source, hostId: 'hub-a', spaceId: 's1' }
  const ctx = new SpaceContext()
  ctx.open(source, 's1')
  await settle()
  const before = ctx.history.entries
  source.connectedClient = null
  source.changed()
  assert.equal(ctx.link, 'offline')
  assert.equal(ctx.history.entries, before)
  // After BotSync's grace expires this store leaves onlineStores; it is still in the visible stores.
  assert.equal(retainedSpace([source], origin), item)
  assert.equal(ctx.spaceId, 's1')
  assert.ok(ctx.asOf)
  assert.equal(retainedSpace([], origin), null, 'a hidden or removed computer is not retained')
  source.spaces = { ...source.spaces, revision: 2, spaces: [] }
  assert.equal(retainedSpace([source], origin), null, 'confirmed list removal closes the view')
  source.spaces = { ...source.spaces, spaces: [item] }
  source.hello = { hostId: 'hub-b', rev: 1, spacesContract: 1 }
  source.changed()
  assert.equal(retainedSpace([source], origin), null)
  assert.equal(ctx.spaceId, null)
})

// R1 (decisão A): a vista de um space lê só o estado da tela que o stream DAQUELE space traz; nunca o do Pessoal.
test('uso do computador: Loja nomeia quem usa quando autorizado; outro space, link velho, offline e pausa não deixam identidade', async () => {
  const screen = (agentUse: unknown, agentBot: string | null) => ({ enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0, agentUse, agentBot })
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  const now = () => ctx.screen
  ctx.open(source, 'loja')
  await settle()
  assert.equal(now(), null, 'antes do hello: nenhum estado, nada de Ready')
  link.emit({ type: 'hello', spaceId: 'loja', screen: screen('using', 'donna') })
  assert.deepEqual([now()?.agentUse, now()?.agentBot], ['using', 'donna'])
  assert.equal(botName(now()!.agentBot!, ctx.list.principal, ctx.bots), 'Donna', 'nome canônico da principal da lista')
  link.emit({ type: 'screen', spaceId: 'pessoal', screen: screen('unavailable', null) })
  link.emit({ type: 'screen', screen: screen('idle', null) })
  assert.equal(now()?.agentBot, 'donna', 'evento de outro space, ou sem space, não pinta aqui')
  link.emit({ type: 'screen', spaceId: 'loja', screen: screen('unavailable', 'donna') })
  assert.deepEqual([now()?.agentUse, now()?.agentBot], ['unavailable', null], 'contraditório vira indisponível sem nome')
  link.emit({ type: 'screen', spaceId: 'loja', screen: { ...screen(undefined, 'donna') } })
  assert.deepEqual([now()?.agentUse, now()?.agentBot], ['unavailable', null], 'Hub antigo sem agentUse')
  link.emit({ type: 'screen', spaceId: 'loja', screen: screen('using', 'donna') })
  source.connectedClient = null
  source.changed()
  assert.equal(now(), null, 'sem conexão: a identidade de antes some')
  const link2 = new FakeLink('hub-a')
  source.connectedClient = link2
  source.principalEpoch = 2
  source.changed()
  link.emit({ type: 'screen', spaceId: 'loja', screen: screen('using', 'donna') })
  assert.equal(now(), null, 'evento do link velho é descartado')
  link2.emit({ type: 'hello', spaceId: 'loja', screen: screen('using', 'donna') })
  assert.equal(now()?.agentBot, 'donna')
  link2.streams.at(-1)!.onEnd(null)
  assert.equal(now(), null, 'atualizações pausadas: sem identidade retida')
  ctx.open(source, 'outro')
  assert.equal(now(), null, 'abrir outro space começa sem estado')
})

// R2: um bot nativo da Loja que já existia antes do hello (rev abaixo dele) chega pelo catch-up do stream do space.
test('uso do computador: bot nativo antigo da Loja é nomeado pelo catch-up; sem roster, sem nome inventado', async () => {
  const screen = (agentUse: string, agentBot: string | null) => ({ enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0, agentUse, agentBot })
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  const name = () => botName(ctx.screen?.agentBot ?? '', ctx.list.principal, ctx.bots)
  ctx.open(source, 'loja')
  await settle()
  assert.equal(link.streams[0]?.since, 0)
  link.emit({ type: 'hello', spaceId: 'loja', screen: screen('using', 'caixa') })
  assert.equal(name(), 'Bot deste space', 'antes do roster: sem nome, nada inferido de título ou lane')
  link.emit({ type: 'bot', spaceId: 'loja', bot: { id: 'caixa', name: 'Caixa', rev: 3 } })
  assert.equal(name(), 'Caixa', 'nome canônico do próprio space, mesmo com rev anterior ao hello (40)')
  link.emit({ type: 'bot', spaceId: 'pessoal', bot: { id: 'caixa', name: 'Outro nome', rev: 41 } })
  link.emit({ type: 'bot', bot: { id: 'caixa', name: 'Sem space', spaceId: 'pessoal', rev: 41 } })
  assert.equal(name(), 'Caixa', 'bot de outro space não renomeia')
  link.emit({ type: 'botDeleted', spaceId: 'loja', id: 'caixa', rev: 42 })
  assert.equal(ctx.bots.has('caixa'), false, 'excluído sai do roster')
})

// R4: mesmo num link que ainda entregue linha depois do fim, o SpaceContext real só conta o stream até o próprio onEnd.
test('uso do computador: onEnd e depois screen tardio da mesma origem: segue pausado e sem identidade; retry vale', async () => {
  const screen = (agentUse: string, agentBot: string | null) => ({ enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0, agentUse, agentBot })
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  const now = () => [ctx.link, ctx.screen?.agentBot ?? null]
  ctx.open(source, 'loja')
  await settle()
  link.emit({ type: 'hello', spaceId: 'loja', screen: screen('using', 'donna') })
  assert.deepEqual(now(), ['live', 'donna'])
  link.streams[0]!.onEnd(null)
  assert.deepEqual(now(), ['paused', null])
  link.streams[0]!.onEvent({ type: 'screen', spaceId: 'loja', screen: screen('using', 'donna') } as never)
  assert.deepEqual(now(), ['paused', null], 'evento tardio do stream encerrado (mesma origem) não reativa nada')
  ctx.retry()
  await settle()
  link.streams.at(-1)!.onEvent({ type: 'screen', spaceId: 'loja', screen: screen('using', 'donna') } as never)
  assert.deepEqual(now(), ['live', 'donna'], 'o stream novo do retry vale')
})

test('BN-03: conversa exata pedida e ausente não cai na principal; chega pelo stream e só ela abre', async () => {
  const link = new FakeLink('hub-a')
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 's1', 'bot-novo', true)
  await settle()
  assert.equal(ctx.missing, true)
  assert.equal(ctx.conversationId, null, 'nem a fixa da principal nem a primeira entram no lugar')
  assert.deepEqual(link.calls.map((c) => c.method), ['donnaConversations'])
  link.emit({ type: 'conversation', spaceId: 's1', conversation: conversation('bot-novo', 's1', { botId: 'bot-novo', primary: true }) })
  await settle()
  assert.equal(ctx.missing, false)
  assert.equal(ctx.conversationId, 'bot-novo')
  assert.deepEqual(link.calls.at(-1)?.body.conversationId, 'bot-novo')
})

// Encaixe BN03/privacidade (D70): trocar a conversa exata reabre o stream do mesmo Space;
// um hello antigo não pode repor identidade de tela nem escolher a conversa antes da origem atual.
test('BN03 + privacidade: conversa exata ausente, roster e Screen respeitam a nova abertura e o fim do stream', async () => {
  const screen = { enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0, agentUse: 'using', agentBot: 'bot-novo' }
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  const state = () => [ctx.conversationId, ctx.screen?.agentBot ?? null]
  openSpaceAt(ctx, source, 's1')
  await settle()
  const old = link.streams[0]!
  old.onEvent({ type: 'screen', spaceId: 's1', screen } as never)
  assert.deepEqual(state(), ['s1-fixa', 'bot-novo'])

  openSpaceAt(ctx, source, 's1', 'bot-novo')
  assert.equal(ctx.history.entries.length, 0, 'a abertura exata limpa o histórico anterior antes de pintar')
  assert.equal(ctx.screen, null, 'a identidade de tela também aguarda a nova observação')
  const target = conversation('bot-novo', 's1', { botId: 'bot-novo', primary: true })
  old.onEvent({ type: 'hello', spaceId: 's1', conversations: [target], screen } as never)
  await settle()
  assert.deepEqual(state(), [null, null], 'hello da abertura anterior não satisfaz a conversa exata nem repõe Screen')
  assert.equal(ctx.missing, true)
  assert.equal(link.streams.at(-1)?.since, 0, 'a conversa exata conserva o catch-up completo do Space')

  link.emit({ type: 'hello', spaceId: 's1', screen })
  link.emit({ type: 'bot', spaceId: 'pessoal', bot: { id: 'bot-novo', name: 'Nome estrangeiro' } })
  assert.equal(botName('bot-novo', ctx.list.principal, ctx.bots), 'Bot deste space')
  link.emit({ type: 'bot', spaceId: 's1', bot: { id: 'bot-novo', name: 'Especialista' } })
  assert.equal(botName('bot-novo', ctx.list.principal, ctx.bots), 'Especialista')
  assert.deepEqual(state(), [null, 'bot-novo'], 'Screen autorizado não escolhe outra conversa no lugar da ausente')
  link.emit({ type: 'conversation', spaceId: 's1', conversation: target })
  await settle()
  assert.deepEqual(state(), ['bot-novo', 'bot-novo'])
  assert.ok(ctx.history.entries.every((e) => e.conversationId === 'bot-novo'))

  const current = link.streams.at(-1)!
  current.onEnd(null)
  current.onEvent({ type: 'hello', spaceId: 's1', conversations: [target], screen } as never)
  assert.deepEqual([ctx.link, ctx.screen], ['paused', null], 'fim definitivo também vale após abrir a conversa da biblioteca')
  ctx.close()
})

test('D543: compositor envia só à conversa canônica do Space capturado, sem cair no Pessoal', async () => {
  const link = new FakeLink('hub-a', async (method, body) => {
    if (method === 'donnaConversations') return { hubId: 'hub-a', spaceId: body.spaceId, conversations: [conversation('bot-a', body.spaceId as string, { botId: 'bot-a', primary: true })] }
    if (method === 'send') {
      // O legado devolve Entry sem os seletores adicionais: o recibo é validado pelo bot canônico.
      const { spaceId: _space, conversationId: _conversation, ...legacy } = entry('enviada', 4, body.spaceId as string, body.botId as string)
      return { entry: legacy }
    }
    return standard('hub-a', method, body)
  })
  const ctx = new SpaceContext()
  ctx.open(new FakeSource(link), 'trabalho')
  await settle()
  const intent = ctx.prepareMessage('Mensagem de trabalho', 'nonce-d543')
  await intent.send()
  const sent = link.calls.filter((c) => c.method === 'send')
  assert.deepEqual(sent.map((c) => c.body), [{ hubId: 'hub-a', spaceId: 'trabalho', conversationId: 'bot-a', botId: 'bot-a', text: 'Mensagem de trabalho', clientNonce: 'nonce-d543' }])
  assert.equal(ctx.history.entries.some((e) => e.id === 'enviada'), true)
})

test('D543: intenção capturada não muda de conversa, Space, Hub ou link, inclusive A→B→A', async () => {
  const link = new FakeLink('hub-a')
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  ctx.open(source, 'A'); await settle()
  const intent = ctx.prepareMessage('Só A', 'nonce-a')
  ctx.select('A-c2'); await settle()
  await assert.rejects(intent.send(), /stale_message_context/)
  ctx.select('A-fixa'); await settle()
  await assert.rejects(intent.send(), /stale_message_context/)
  const beforeLink = ctx.prepareMessage('Mesmo link', 'nonce-link')
  source.connectedClient = new FakeLink('hub-a'); source.changed(); await settle()
  await assert.rejects(beforeLink.send(), /stale_message_context/)
  const beforeSpace = ctx.prepareMessage('Só A', 'nonce-space')
  ctx.open(source, 'B'); await settle()
  await assert.rejects(beforeSpace.send(), /stale_message_context/)
  const beforeHub = ctx.prepareMessage('Só hub-a', 'nonce-hub')
  source.hello = { hostId: 'hub-b', spacesContract: 1, rev: 1 }; source.changed()
  await assert.rejects(beforeHub.send(), /stale_message_context/)
  assert.equal(link.calls.some((c) => c.method === 'send' || c.method === 'donnaConversationSend'), false)
})

test('D543: envio adicional leva revisão e nonce; resposta atrasada não pinta outro Space', async () => {
  const pending = deferred<unknown>()
  const link = new FakeLink('hub-a', (method, body) => method === 'donnaConversationSend' ? pending.promise : Promise.resolve(standard('hub-a', method, body)))
  const source = new FakeSource(link)
  const ctx = new SpaceContext()
  ctx.open(source, 'A'); await settle()
  const intent = ctx.prepareMessage('Adicional', 'nonce-extra')
  const sending = intent.send()
  const body = link.calls.find((c) => c.method === 'donnaConversationSend')!.body
  assert.deepEqual([body.hubId, body.spaceId, body.conversationId, body.expectedRevision, body.clientNonce], ['hub-a', 'A', 'A-fixa', 1, 'nonce-extra'])
  ctx.open(source, 'B'); await settle()
  pending.resolve({ hubId: 'hub-a', context: { spaceId: 'A', conversationId: 'A-fixa' }, entry: entry('tardia', 5, 'A', 'A-fixa') })
  await assert.rejects(sending)
  assert.equal(ctx.history.entries.some((e) => e.id === 'tardia'), false)
})

test('D543: retry explícito conserva nonce; atualização do próprio envio não produz falso erro', async () => {
  const link = new FakeLink('hub-a')
  const ctx = new SpaceContext()
  let attempts = 0
  link.answer = async (method, body) => {
    if (method === 'donnaConversationSend') {
      attempts++
      if (attempts === 1) throw Object.assign(new Error('lost_ack'), { status: 0 })
      link.emit({ type: 'conversation', spaceId: 'A', conversation: conversation('A-fixa', 'A', { revision: 2, stateRevision: 2 }) })
      return { hubId: 'hub-a', context: { spaceId: 'A', conversationId: 'A-fixa' }, entry: entry('replay-confirmado', 6, 'A', 'A-fixa') }
    }
    return standard('hub-a', method, body)
  }
  ctx.open(new FakeSource(link), 'A'); await settle()
  const intent = ctx.prepareMessage('Não duplicar', 'nonce-repetido')
  await assert.rejects(intent.send(), /lost_ack/)
  await intent.send()
  const requests = link.calls.filter((c) => c.method === 'donnaConversationSend')
  assert.deepEqual(requests[0]!.body, requests[1]!.body)
  assert.equal(ctx.history.entries.some((e) => e.id === 'replay-confirmado'), true)
})

test('D543 E1: ativação relê principal e conversa com o Space já aberto', async () => {
  let active = false
  const link = new FakeLink('hub-a', async (method, body) => {
    if (method === 'donnaConversations') return { hubId: 'hub-a', spaceId: 's1', principal: active ? { id: 'berga-s1', name: 'Berga' } : null,
      conversations: active ? [conversation('berga-s1', 's1', { botId: 'berga-s1', primary: true })] : [] }
    return standard('hub-a', method, body)
  })
  const source = new FakeSource(link)
  const snapshot = (principalBotId: string | null): SpacesSnapshot => ({ version: 1, revision: active ? 2 : 1, personalSpaceId: 'personal', spaces: [
    { id: 's1', kind: 'space', name: 'Estúdio', position: 1, revision: 1, principalBotId, tile: null, appearanceConfigured: false,
      appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null },
  ] })
  source.spaces = snapshot(null)
  const ctx = new SpaceContext()
  ctx.open(source, 's1')
  await settle()
  assert.equal(ctx.list.principal, null)
  assert.equal(ctx.list.conversations.length, 0)
  active = true
  source.spaces = snapshot('berga-s1')
  source.changed()
  await settle()
  assert.deepEqual(ctx.list.principal, { id: 'berga-s1', name: 'Berga' })
  assert.equal(ctx.conversationId, 'berga-s1')
  assert.equal(ctx.list.conversations.length, 1)
  ctx.close()
})

test('D543 E1: rótulos distinguem Berga do Pessoal e principal do Space', () => {
  const foreign = { personalId: 'personal-berga', spaces: new Map([['space-berga', 'Estúdio']]) }
  assert.equal(botName('personal-berga', { id: 'space-berga', name: 'Berga' }, new Map(), foreign), 'Berga do Pessoal')
  assert.equal(botName('space-berga', { id: 'space-berga', name: 'Berga' }, new Map(), foreign), 'Berga')
})
