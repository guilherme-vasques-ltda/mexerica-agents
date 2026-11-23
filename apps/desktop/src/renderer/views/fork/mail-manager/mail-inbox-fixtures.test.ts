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

test('última abertura de fio vence e somente a abertura vigente conclui o loading', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const first = deferred<typeof fixtures.detail>()
  const second = deferred<typeof fixtures.detail>()
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async () => ({ threads: [
      ...fixtures.threads.threads,
      { ...fixtures.threads.threads[0]!, threadId: 'thread-second', subject: 'Segundo' },
    ], nextCursor: null }),
    openThread: async (_space, _mailbox, threadId) => threadId === 'thread-demo' ? first.promise : second.promise,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  const openingFirst = state.openThread('thread-demo')
  const openingSecond = state.openThread('thread-second')
  second.resolve({ ...fixtures.detail, threadId: 'thread-second' })
  await openingSecond
  assert.equal(state.thread?.threadId, 'thread-second')
  assert.equal(state.loadingThread, false)
  first.resolve(fixtures.detail)
  await openingFirst
  assert.equal(state.thread?.threadId, 'thread-second')
  assert.equal(state.loadingThread, false)
})

test('troca de lista invalida uma abertura pendente', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const opening = deferred<typeof fixtures.detail>()
  const gateway: MailInboxGateway = {
    status: async () => ({ state: 'ready', mailboxes: [
      { mailboxId: 'mailbox-a', label: 'Caixa A' }, { mailboxId: 'mailbox-b', label: 'Caixa B' },
    ] }),
    listThreads: async (_space, mailbox) => ({
      threads: [{ ...fixtures.threads.threads[0]!, threadId: `thread-${mailbox}` }],
      nextCursor: null,
    }),
    openThread: async () => opening.promise,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  const pending = state.openThread('thread-mailbox-a')
  await state.selectMailbox('mailbox-b')
  opening.resolve(fixtures.detail)
  await pending
  assert.equal(state.mailboxId, 'mailbox-b')
  assert.equal(state.thread, null)
  assert.equal(state.loadingThread, false)
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

test('refresh sem contexto não consulta gateway e limpa os dados; id vazio também nega', async () => {
  let calls = 0
  const gateway: MailInboxGateway = {
    status: async () => { calls++; return fixtures.ready },
    listThreads: async () => { calls++; return fixtures.threads },
    openThread: async () => { calls++; return fixtures.detail },
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  await state.openThread('thread-demo')
  calls = 0
  await state.setContext(null)
  assert.equal(state.status, 'unavailable')
  assert.equal(state.error, 'context_unavailable')
  assert.equal(state.mailboxId, null)
  assert.deepEqual(state.mailboxes, [])
  assert.deepEqual(state.threads, [])
  assert.equal(state.thread, null)
  await state.refresh('')
  assert.equal(calls, 0)
})

test('cursor inválido só reinicia uma página posterior; primeira página inválida encerra sem loop', async () => {
  let pageCalls = 0
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async (_spaceId, _mailboxId, cursor) => {
      pageCalls++
      if (cursor) throw { code: 'cursor_invalid' }
      return { ...fixtures.threads, nextCursor: 'next' }
    },
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  await state.loadNextPage()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(pageCalls, 3)
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-demo'])
  assert.equal(state.error, null, 'recuperação bem-sucedida limpa o erro do cursor')

  let firstPageCalls = 0
  const brokenGateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async () => { firstPageCalls++; throw { code: 'cursor_invalid' } },
    openThread: async () => fixtures.detail,
  }
  const broken = new MailInboxReadState(brokenGateway, 'space-a')
  await broken.refresh()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(firstPageCalls, 1)
  assert.equal(broken.status, 'unavailable')
  assert.equal(broken.error, 'cursor_invalid', 'erro terminal permanece visível')
  assert.deepEqual(broken.threads, [])
})

test('paginação elimina fios duplicados pelo id estável', async () => {
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async (_spaceId, _mailboxId, cursor) => cursor
      ? { threads: [...fixtures.threads.threads, { ...fixtures.threads.threads[0]!, threadId: 'thread-second' }], nextCursor: null }
      : { ...fixtures.threads, nextCursor: 'next' },
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  await state.loadNextPage()
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-demo', 'thread-second'])
})

test('página deduplica também IDs repetidos dentro dela', async () => {
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async (_space, _mailbox, cursor) => cursor
      ? { threads: [
        { ...fixtures.threads.threads[0]!, threadId: 'thread-new' },
        { ...fixtures.threads.threads[0]!, threadId: 'thread-new' },
      ], nextCursor: null }
      : { ...fixtures.threads, nextCursor: 'next' },
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  await state.loadNextPage()
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-demo', 'thread-new'])
})

test('selecionar outra caixa preserva a lista de caixas e a mailbox selecionada', async () => {
  const gateway: MailInboxGateway = {
    status: async () => ({ state: 'ready', mailboxes: [
      { mailboxId: 'mailbox-a', label: 'Caixa A' }, { mailboxId: 'mailbox-b', label: 'Caixa B' },
    ] }),
    listThreads: async (_space, mailbox) => ({
      threads: [{ ...fixtures.threads.threads[0]!, threadId: `thread-${mailbox}` }],
      nextCursor: null,
    }),
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  await state.selectMailbox('mailbox-b')
  assert.equal(state.mailboxId, 'mailbox-b')
  assert.deepEqual(state.mailboxes.map((mailbox) => mailbox.mailboxId), ['mailbox-a', 'mailbox-b'])
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-mailbox-b'])
})

test('troca de caixa limpa lista e cursor antes da leitura pendente, bloqueia fio/cursor antigos e depois mostra B', async () => {
  const deferred = <T,>() => {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((r) => { resolve = r })
    return { promise, resolve }
  }
  const pageB = deferred<{ threads: typeof fixtures.threads.threads; nextCursor: string | null }>()
  const calls: string[] = []
  const gateway: MailInboxGateway = {
    status: async () => ({ state: 'ready', mailboxes: [
      { mailboxId: 'mailbox-a', label: 'Caixa A' }, { mailboxId: 'mailbox-b', label: 'Caixa B' },
    ] }),
    listThreads: async (_space, mailbox, cursor) => {
      calls.push(`list:${mailbox}:${cursor ?? 'first'}`)
      if (mailbox === 'mailbox-a') return {
        threads: [{ ...fixtures.threads.threads[0]!, threadId: 'thread-a' }],
        nextCursor: 'cursor-a',
      }
      return pageB.promise
    },
    openThread: async (_space, mailbox, threadId) => {
      calls.push(`open:${mailbox}:${threadId}`)
      return { ...fixtures.detail, threadId }
    },
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  await state.refresh()
  assert.equal(state.nextCursor, 'cursor-a')
  const selectB = state.selectMailbox('mailbox-b')
  assert.equal(state.mailboxId, 'mailbox-b')
  assert.deepEqual(state.mailboxes.map((mailbox) => mailbox.mailboxId), ['mailbox-a', 'mailbox-b'])
  assert.deepEqual(state.threads, [])
  assert.equal(state.nextCursor, null)
  assert.equal(state.thread, null)
  assert.equal(state.loadingPage, true)
  assert.equal(state.status, 'ready')

  const callCountBeforeInvalidActions = calls.length
  await state.openThread('thread-a')
  await state.loadNextPage()
  assert.equal(calls.length, callCountBeforeInvalidActions)
  assert.ok(!calls.includes('open:mailbox-b:thread-a'))
  assert.ok(!calls.includes('list:mailbox-b:cursor-a'))

  pageB.resolve({ threads: [{ ...fixtures.threads.threads[0]!, threadId: 'thread-b' }], nextCursor: 'cursor-b' })
  await selectB
  const threadsAfterSwitch: { threadId: string }[] = state.threads
  assert.deepEqual(threadsAfterSwitch.map((thread) => thread.threadId), ['thread-b'])
  assert.equal(state.nextCursor, 'cursor-b')
  assert.equal(state.loadingPage, false)
})

test('status mapeia not_connected e reconnect_required com controle positivo de pronto', async () => {
  for (const code of ['not_connected', 'reconnect_required']) {
    const state = new MailInboxReadState({
      status: async () => { throw { code } },
      listThreads: async () => fixtures.threads,
      openThread: async () => fixtures.detail,
    }, 'space-a')
    await state.refresh()
    assert.equal(state.status, code)
    assert.deepEqual(state.mailboxes, [])
  }
  const ready = new MailInboxReadState(new FixtureMailInboxGateway(fixtureText), 'space-a')
  await ready.refresh()
  assert.equal(ready.status, 'ready')
  assert.equal(ready.threads.length, 1)
})

test('primeira página sinaliza conclusão para observadores, inclusive recuperação do cursor', async () => {
  const gateway: MailInboxGateway = {
    status: async () => fixtures.ready,
    listThreads: async (_space, _mailbox, cursor) => cursor
      ? Promise.reject({ code: 'cursor_invalid' })
      : { ...fixtures.threads, nextCursor: 'next' },
    openThread: async () => fixtures.detail,
  }
  const state = new MailInboxReadState(gateway, 'space-a')
  let changes = 0
  state.subscribe(() => { changes++ })
  await state.refresh()
  assert.ok(changes >= 3)
  const before = changes
  await state.loadNextPage()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.ok(changes > before)
  assert.equal(state.loadingPage, false)
  assert.deepEqual(state.threads.map((thread) => thread.threadId), ['thread-demo'])
})
