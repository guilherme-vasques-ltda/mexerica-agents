import { Observable } from '../../../lib/observable.ts'

export type InboxStatus =
  | { state: 'ready'; mailboxes: { mailboxId: string; label: string }[] }
  | { state: 'not_connected' | 'reconnect_required' | 'unavailable' }

export interface InboxThread {
  threadId: string
  subject: string | null
  lastMessageAt: string
}

export interface InboxMessage {
  messageId: string
  fromDisplay: string | null
  fromDomain: string | null
  sentAt: string
  bodyText: string
}

export interface InboxThreadDetail {
  threadId: string
  subject: string | null
  messages: InboxMessage[]
  truncated: boolean
}

function deduplicateThreads(threads: InboxThread[]): InboxThread[] {
  const seen = new Set<string>()
  return threads.filter((thread) => {
    if (seen.has(thread.threadId)) return false
    seen.add(thread.threadId)
    return true
  })
}

export const inboxFixtures = (t: (key: 'mailInbox.fixtureMailbox' | 'mailInbox.fixtureSubject' | 'mailInbox.fixtureSender' | 'mailInbox.fixtureBody') => string) => ({
  ready: {
    state: 'ready',
    mailboxes: [{ mailboxId: 'mailbox-demo', label: t('mailInbox.fixtureMailbox') }],
  },
  empty: { threads: [], nextCursor: null },
  threads: {
    threads: [{ threadId: 'thread-demo', subject: t('mailInbox.fixtureSubject'), lastMessageAt: '2026-11-22T14:30:00Z' }],
    nextCursor: null,
  },
  detail: {
    threadId: 'thread-demo',
    subject: t('mailInbox.fixtureSubject'),
    messages: [{
      messageId: 'message-demo',
      fromDisplay: t('mailInbox.fixtureSender'),
      fromDomain: 'loja.example.invalid',
      sentAt: '2026-11-22T14:30:00Z',
      bodyText: t('mailInbox.fixtureBody'),
    }],
    truncated: false,
  },
} satisfies {
  ready: InboxStatus
  empty: { threads: InboxThread[]; nextCursor: string | null }
  threads: { threads: InboxThread[]; nextCursor: string | null }
  detail: InboxThreadDetail
})

export const inboxStatusFixtures = {
  notConnected: { state: 'not_connected' },
  reconnectRequired: { state: 'reconnect_required' },
  unavailable: { state: 'unavailable' },
} satisfies Record<string, InboxStatus>

export const inboxErrorFixtures = {
  invalid_request: { code: 'invalid_request' },
  access_denied: { code: 'access_denied' },
  not_connected: { code: 'not_connected' },
  reconnect_required: { code: 'reconnect_required' },
  unavailable: { code: 'unavailable' },
  cursor_invalid: { code: 'cursor_invalid' },
} as const

export const mailReadMcpAllowlist = ['search_threads', 'get_thread', 'get_message', 'list_labels'] as const

export function mayUseMailReadMcpTool(tool: string): boolean {
  return (mailReadMcpAllowlist as readonly string[]).includes(tool)
}

export interface MailInboxGateway {
  status(spaceId: string): Promise<InboxStatus>
  listThreads(spaceId: string, mailboxId: string, cursor?: string): Promise<{ threads: InboxThread[]; nextCursor: string | null }>
  openThread(spaceId: string, mailboxId: string, threadId: string): Promise<InboxThreadDetail>
}

export class FixtureMailInboxGateway implements MailInboxGateway {
  readonly #fixtures: ReturnType<typeof inboxFixtures>

  constructor(translate: (key: 'mailInbox.fixtureMailbox' | 'mailInbox.fixtureSubject' | 'mailInbox.fixtureSender' | 'mailInbox.fixtureBody') => string) {
    this.#fixtures = inboxFixtures(translate)
  }

  async status(_spaceId: string): Promise<InboxStatus> { return structuredClone(this.#fixtures.ready) }

  async listThreads(_spaceId: string, _mailboxId: string, cursor?: string) {
    return structuredClone(cursor ? { threads: [], nextCursor: null } : this.#fixtures.threads)
  }

  async openThread(_spaceId: string, _mailboxId: string, _threadId: string) {
    return structuredClone(this.#fixtures.detail)
  }
}

export type InboxStatusState = InboxStatus['state'] | 'loading'

export class MailInboxReadState extends Observable {
  status: InboxStatusState = 'loading'
  mailboxes: { mailboxId: string; label: string }[] = []
  mailboxId: string | null = null
  threads: InboxThread[] = []
  nextCursor: string | null = null
  thread: InboxThreadDetail | null = null
  loadingPage = false
  loadingThread = false
  error: string | null = null
  #generation = 0
  #listGeneration = 0
  #openGeneration = 0
  readonly #gateway: MailInboxGateway
  #spaceId: string

  constructor(gateway: MailInboxGateway, spaceId: string) {
    super()
    this.#gateway = gateway
    this.#spaceId = spaceId
  }

  async refresh(spaceId = this.#spaceId): Promise<void> {
    if (!spaceId) {
      ++this.#generation
      ++this.#listGeneration
      ++this.#openGeneration
      this.#spaceId = ''
      this.status = 'unavailable'
      this.error = 'context_unavailable'
      this.#clearContent()
      this.#changed()
      return
    }
    this.#spaceId = spaceId
    const generation = ++this.#generation
    ++this.#listGeneration
    ++this.#openGeneration
    this.#retryingInvalidCursor = false
    this.#clearContent()
    this.status = 'loading'
    this.error = null
    this.#changed()
    try {
      const status = await this.#gateway.status(spaceId)
      if (generation !== this.#generation) return
      this.status = status.state
      this.mailboxes = status.state === 'ready' ? status.mailboxes : []
      if (status.state === 'ready') {
        this.mailboxId = status.mailboxes[0]?.mailboxId ?? null
        if (this.mailboxId) await this.#loadFirstPage(generation)
      }
      this.#changed()
    } catch (error) {
      if (generation !== this.#generation) return
      this.#handleError(error, false)
      this.#changed()
    }
  }

  setContext(spaceId: string | null): Promise<void> {
    if (spaceId === null || !spaceId) return this.refresh('')
    return this.refresh(spaceId)
  }

  async selectMailbox(mailboxId: string): Promise<void> {
    if (this.status !== 'ready' || !this.mailboxes.some((mailbox) => mailbox.mailboxId === mailboxId)) return
    this.mailboxId = mailboxId
    this.#retryingInvalidCursor = false
    const generation = this.#generation
    ++this.#listGeneration
    ++this.#openGeneration
    this.threads = []
    this.nextCursor = null
    this.error = null
    this.#clearThreadContent()
    this.loadingPage = true
    this.#changed()
    await this.#loadFirstPage(generation)
  }

  async loadNextPage(): Promise<void> {
    const cursor = this.nextCursor
    const mailboxId = this.mailboxId
    if (!cursor || !mailboxId || this.status !== 'ready' || this.loadingPage) return
    const generation = this.#generation
    const listGeneration = this.#listGeneration
    this.loadingPage = true
    this.#changed()
    this.error = null
    try {
      const result = await this.#gateway.listThreads(this.#spaceId, mailboxId, cursor)
      if (generation !== this.#generation || listGeneration !== this.#listGeneration || mailboxId !== this.mailboxId) return
      const known = new Set(this.threads.map((thread) => thread.threadId))
      this.threads = [...this.threads, ...deduplicateThreads(result.threads).filter((thread) => !known.has(thread.threadId))]
      this.nextCursor = result.nextCursor
      this.#changed()
    } catch (error) {
      if (generation === this.#generation && listGeneration === this.#listGeneration) { this.#handleError(error, true); this.#changed() }
    } finally {
      if (generation === this.#generation && listGeneration === this.#listGeneration) { this.loadingPage = false; this.#changed() }
    }
  }

  async openThread(threadId: string): Promise<void> {
    const mailboxId = this.mailboxId
    if (!mailboxId || this.status !== 'ready' || !this.threads.some((thread) => thread.threadId === threadId)) return
    const generation = this.#generation
    const listGeneration = this.#listGeneration
    const openGeneration = ++this.#openGeneration
    this.thread = null
    this.loadingThread = true
    this.error = null
    this.#changed()
    try {
      const thread = await this.#gateway.openThread(this.#spaceId, mailboxId, threadId)
      if (generation !== this.#generation || listGeneration !== this.#listGeneration || openGeneration !== this.#openGeneration || mailboxId !== this.mailboxId) return
      this.thread = thread
      this.#changed()
    } catch (error) {
      if (generation === this.#generation && listGeneration === this.#listGeneration && openGeneration === this.#openGeneration) { this.#handleError(error, false); this.#changed() }
    } finally {
      if (generation === this.#generation && listGeneration === this.#listGeneration && openGeneration === this.#openGeneration) { this.loadingThread = false; this.#changed() }
    }
  }

  #clearContent() {
    this.mailboxes = []
    this.mailboxId = null
    this.threads = []
    this.nextCursor = null
    this.thread = null
    this.loadingPage = false
    this.#clearThreadContent()
  }

  #clearThreadContent() {
    this.thread = null
    this.loadingThread = false
  }

  async #loadFirstPage(generation: number) {
    const mailboxId = this.mailboxId
    if (!mailboxId) return
    const listGeneration = ++this.#listGeneration
    this.loadingPage = true
    try {
      const result = await this.#gateway.listThreads(this.#spaceId, mailboxId)
      if (generation !== this.#generation || listGeneration !== this.#listGeneration || mailboxId !== this.mailboxId) return
      this.threads = deduplicateThreads(result.threads)
      this.nextCursor = result.nextCursor
      this.error = null
      this.#retryingInvalidCursor = false
      this.#changed()
    } catch (error) {
      if (generation === this.#generation && listGeneration === this.#listGeneration) {
        this.#handleError(error, false)
        this.#changed()
      }
    } finally {
      if (generation === this.#generation && listGeneration === this.#listGeneration) {
        this.loadingPage = false
        this.#changed()
      }
    }
  }

  #handleError(error: unknown, fromPage: boolean) {
    this.error = errorCode(error)
    if (this.error === 'cursor_invalid') {
      if (!fromPage || this.#retryingInvalidCursor) {
        this.status = 'unavailable'
        ++this.#generation
        this.#clearContent()
        return
      }
      this.#listGeneration++
      this.threads = []
      this.nextCursor = null
      this.thread = null
      this.#retryingInvalidCursor = true
      void this.#loadFirstPage(this.#generation)
      return
    }
    if (['access_denied', 'not_connected', 'reconnect_required', 'unavailable'].includes(this.error)) {
      this.status = this.error === 'access_denied' ? 'unavailable' : this.error as InboxStatusState
      ++this.#generation
      ++this.#listGeneration
      ++this.#openGeneration
      this.#clearContent()
      return
    }
    this.status = 'unavailable'
    ++this.#generation
    ++this.#listGeneration
    ++this.#openGeneration
    this.#clearContent()
  }

  #changed() {
    this.changed()
  }

  #retryingInvalidCursor = false
}

function errorCode(error: unknown): string {
  if (error && typeof error === 'object' && 'code' in error) return String(error.code)
  return 'unavailable'
}
