import assert from 'node:assert/strict'
import test from 'node:test'
import { FixtureMailInboxGateway, inboxErrorFixtures, inboxFixtures, inboxStatusFixtures, MailInboxReadState, mayUseMailReadMcpTool, mailReadMcpAllowlist, type MailInboxGateway } from './mail-inbox-fixtures.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'

const t = tradutor('pt-BR')
const fixtureText = (key: 'mailInbox.fixtureMailbox' | 'mailInbox.fixtureSubject' | 'mailInbox.fixtureSender' | 'mailInbox.fixtureBody') => t(key)
const fixtures = inboxFixtures(fixtureText)
test('fixtures de leitura distinguem estado indisponível de uma caixa pronta e vazia', () => {
  assert.deepEqual(inboxStatusFixtures.notConnected, { state: 'not_connected' })
  assert.deepEqual(inboxStatusFixtures.reconnectRequired, { state: 'reconnect_required' })
  assert.deepEqual(inboxStatusFixtures.unavailable, { state: 'unavailable' })
  assert.deepEqual(fixtures.ready, {
    state: 'ready',
    mailboxes: [{ mailboxId: 'mailbox-demo', label: 'Suporte fictício' }],
  })
  assert.deepEqual(fixtures.empty, { threads: [], nextCursor: null })
})

test('fixtures incluem lista e abertura de fio só com campos de leitura', () => {
  assert.deepEqual(Object.keys(fixtures.threads.threads[0]!).sort(), ['lastMessageAt', 'subject', 'threadId'])
  assert.deepEqual(Object.keys(fixtures.detail).sort(), ['messages', 'subject', 'threadId', 'truncated'])
  assert.deepEqual(Object.keys(fixtures.detail.messages[0]!).sort(), ['bodyText', 'fromDisplay', 'fromDomain', 'messageId', 'sentAt'])
  assert.equal(fixtures.detail.messages[0]!.fromDomain, 'loja.example.invalid')
  assert.match(fixtures.detail.messages[0]!.bodyText, /https:\/\/example\.invalid\/link/)
  assert.equal(fixtures.detail.truncated, false)
})

test('erros propostos têm cenários sintéticos sem chamada RPC', () => {
  assert.deepEqual(Object.keys(inboxErrorFixtures).sort(), [
    'access_denied', 'cursor_invalid', 'invalid_request', 'not_connected', 'reconnect_required', 'unavailable',
  ])
})

test('política local aceita apenas os quatro nomes MCP curados', () => {
  assert.deepEqual(mailReadMcpAllowlist, ['search_threads', 'get_thread', 'get_message', 'list_labels'])
  for (const tool of mailReadMcpAllowlist) assert.equal(mayUseMailReadMcpTool(tool), true)
  for (const tool of ['send_message', 'create_draft', 'trash_message', 'gmail.search', 'unknown']) assert.equal(mayUseMailReadMcpTool(tool), false)
})

test('estado de leitura carrega fixture, pagina e abre fio sem operações de escrita', async () => {
  const state = new MailInboxReadState(new FixtureMailInboxGateway(fixtureText), 'space-demo')
  await state.refresh()
  assert.equal(state.status, 'ready')
  assert.equal(state.threads[0]?.threadId, 'thread-demo')
  await state.openThread('thread-demo')
  assert.match(state.thread?.messages[0]?.bodyText ?? '', /texto inerte/)
})

test('desconexão limpa conteúdo e descarta respostas antigas, inclusive paginação concorrente ao refresh', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const oldPage = deferred<{ threads: typeof fixtures.threads.threads; nextCursor: string | null }>()
  let listCalls = 0
  let disconnected = false
  const gateway: MailInboxGateway = {
    status: async () => disconnected ? { state: 'not_connected' } : fixtures.ready,
    listThreads: async (_spaceId, _mailboxId, cursor) => {
      listCalls++
      if (cursor === 'next') return oldPage.promise
      return { threads: fixtures.threads.threads, nextCursor: 'next' }
    },
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-demo')
  await state.refresh()
  await state.openThread('thread-demo')
  const paging = state.loadNextPage()
  disconnected = true
  await state.refresh()
  oldPage.resolve({ threads: fixtures.threads.threads, nextCursor: null })
  await paging
  assert.equal(state.status, 'not_connected')
  assert.deepEqual(state.threads, [])
  assert.equal(state.thread, null)
  assert.equal(state.nextCursor, null)
  assert.equal(listCalls, 2)
})

test('negação durante abertura limpa fio e lista', async () => {
  const opening = new Promise<never>((_resolve, reject) => setTimeout(() => reject({ code: 'access_denied' }), 0))
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async () => fixtures.threads,
    openThread: async () => opening,
  }
  const state = new MailInboxReadState(gateway, 'space-demo')
  await state.refresh()
  await state.openThread('thread-demo')
  assert.equal(state.status, 'unavailable')
  assert.equal(state.thread, null)
  assert.deepEqual(state.threads, [])
})

test('troca de Space e perda de contexto invalidam leituras em voo e limpam seleção/dados', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const delayedOpen = deferred<typeof fixtures.detail>()
  let delay = false
  const gateway: MailInboxGateway = {
    status: async (spaceId) => spaceId === 'space-b' ? { state: 'not_connected' } : fixtures.ready,
    listThreads: async () => fixtures.threads,
    openThread: async () => delay ? delayedOpen.promise : fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  delay = true
  const opening = state.openThread('thread-demo')
  await state.setContext('space-b')
  delayedOpen.resolve(fixtures.detail)
  await opening
  assert.equal(state.status, 'not_connected')
  assert.equal(state.thread, null)
  assert.deepEqual(state.threads, [])
  assert.equal(state.mailboxId, null)
  await state.setContext(null)
  assert.equal(state.status, 'unavailable')
  assert.deepEqual(state.mailboxes, [])
  assert.equal(state.error, 'context_unavailable')
})

test('refresh na mesma caixa descarta paginação pendente e substitui a lista', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const delayedPage = deferred<{ threads: typeof fixtures.threads.threads; nextCursor: string | null }>()
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async (_spaceId, _mailboxId, cursor) => cursor ? delayedPage.promise : fixtures.threads,
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  const paging = state.loadNextPage()
  await state.refresh()
  delayedPage.resolve({ threads: [{ ...fixtures.threads.threads[0]!, threadId: 'stale' }], nextCursor: null })
  await paging
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-demo'])
  assert.equal(state.nextCursor, null)
})
