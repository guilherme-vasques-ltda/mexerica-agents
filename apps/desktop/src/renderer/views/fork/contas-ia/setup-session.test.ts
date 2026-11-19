import assert from 'node:assert/strict'
import test, { mock } from 'node:test'
import type { HostClient } from '../../../client/host-client'
import type { SetupAction, SetupOperation, StartSetup } from '../../../../shared/fork-engine-setup.ts'
import { SetupSession, type OperationRef, type SetupApi } from './setup-session.ts'
import { errorMessage } from './regras.ts'

const ACCOUNT = `ac_${'b'.repeat(64)}`
const client = () => Promise.resolve({} as HostClient)
const op = (patch: Partial<SetupOperation> = {}): SetupOperation => ({
  operationId: 'op-1', engineAccountId: ACCOUNT, generation: 1, phase: 'preparing', revision: 1, expiresAt: 0,
  errorCode: null, challenge: null, terminal: null, ...patch,
})
const flush = async (times = 6) => { for (let i = 0; i < times; i++) await new Promise((r) => setImmediate(r)) }

/** Hub falso: registra pedidos e responde pelo roteiro de cada teste. Nenhum login, CLI ou modelo real. */
function fakeHub(reply: (kind: 'start' | 'action', body: StartSetup | SetupAction) => SetupOperation | Error) {
  const calls: { kind: 'start' | 'action'; body: StartSetup | SetupAction }[] = []
  const answer = async (kind: 'start' | 'action', body: StartSetup | SetupAction) => {
    calls.push({ kind, body })
    const result = reply(kind, body)
    if (result instanceof Error) throw result
    return result
  }
  const api: SetupApi = { start: (_c, body) => answer('start', body), action: (_c, body) => answer('action', body) }
  return { api, calls }
}

test('instala antes quando falta o programa e segue para o login; ao conectar, apaga desafio e saída', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    let reads = 0
    const hub = fakeHub((kind, body) => {
      if (kind === 'start') {
        const start = body as StartSetup
        return start.action === 'install' ? op({ operationId: 'inst', phase: 'connected' }) : op({ operationId: 'login', phase: 'awaiting_authorization', revision: 2,
          challenge: { kind: 'device_code', url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' } })
      }
      reads++
      return reads < 2 ? op({ operationId: 'login', phase: 'awaiting_authorization', revision: 3, challenge: { kind: 'device_code', url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' } })
        : op({ operationId: 'login', phase: 'connected', revision: 4 })
    })
    const refs: (OperationRef | null)[] = []
    const finished: string[] = []
    const session = new SetupSession(ACCOUNT, () => 7, client, hub.api, { installFirst: true, onRef: (r) => refs.push(r), onFinished: (o, step) => finished.push(`${step}:${o.phase}`) })
    session.begin()
    session.begin()
    await flush()
    const starts = hub.calls.filter((c) => c.kind === 'start').map((c) => c.body as StartSetup)
    assert.deepEqual(starts.map((s) => s.action), ['install', 'login'])
    assert.ok(starts.every((s) => s.expectedRevision === 7 && s.engineAccountId === ACCOUNT))
    assert.notEqual(starts[0].clientRequestId, starts[1].clientRequestId)
    assert.equal(session.operation?.challenge?.code, 'ABCD-1234')
    assert.equal(session.reached, 1)
    mock.timers.tick(1500)
    await flush()
    mock.timers.tick(1500)
    await flush()
    assert.equal(session.phase, 'connected')
    assert.equal(session.operation?.challenge, null)
    assert.deepEqual(finished, ['install:connected', 'login:connected'])
    assert.equal(refs.at(-1), null)
    const before = hub.calls.length
    mock.timers.tick(10_000)
    await flush()
    assert.equal(hub.calls.length, before, 'operação final não é mais lida')
  } finally { mock.timers.reset() }
})

test('cancelar só diz cancelado quando o Hub confirma; o desafio some e nada mais é lido', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    let cancelled = false
    const challenge = { kind: 'paste_code' as const, url: 'https://claude.ai/oauth/authorize', displayDomain: 'claude.ai' }
    const hub = fakeHub((kind, body) => {
      if (kind === 'start') return op({ phase: 'awaiting_authorization', challenge })
      const action = (body as SetupAction).action
      if (action === 'cancel') { cancelled = true; return op({ phase: 'awaiting_authorization', revision: 2, challenge }) }
      return cancelled ? op({ phase: 'cancelled', revision: 3 }) : op({ phase: 'awaiting_authorization', revision: 2, challenge })
    })
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
    session.begin()
    await flush()
    void session.cancel()
    await flush()
    assert.equal(session.cancelling, true)
    assert.equal(session.phase, 'awaiting_authorization', 'resposta do cancel ainda não confirmou')
    mock.timers.tick(1500)
    await flush()
    assert.equal(session.phase, 'cancelled')
    assert.equal(session.cancelling, false)
    assert.equal(session.operation?.challenge, null)
    const before = hub.calls.length
    mock.timers.tick(10_000)
    await flush()
    assert.equal(hub.calls.length, before)
  } finally { mock.timers.reset() }
})

test('resposta velha (revisão ou geração menor) e de outra conta não voltam o estado', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    const answers = [
      op({ phase: 'verifying', revision: 5, generation: 2 }),
      op({ phase: 'awaiting_authorization', revision: 4, generation: 2 }),
      op({ phase: 'awaiting_authorization', revision: 9, generation: 1 }),
      op({ phase: 'connected', revision: 9, generation: 2, engineAccountId: `ac_${'c'.repeat(64)}` }),
    ]
    const hub = fakeHub((kind) => kind === 'start' ? op({ phase: 'preparing', revision: 1, generation: 2 }) : answers.shift() ?? op({ phase: 'verifying', revision: 5, generation: 2 }))
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
    session.begin()
    await flush()
    for (let i = 0; i < 4; i++) {
      mock.timers.tick(1500)
      await flush()
      assert.equal(session.phase, 'verifying', `leitura ${i}`)
    }
    session.dispose()
  } finally { mock.timers.reset() }
})

test('fechar o painel para de acompanhar e apaga desafio e saída, sem cancelar a operação no Hub', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    const hub = fakeHub((kind) => kind === 'start'
      ? op({ phase: 'awaiting_authorization', challenge: { kind: 'device_code', url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'WXYZ-0000' }, terminal: { available: true, cursor: 5, output: 'saída' } })
      : op({ phase: 'awaiting_authorization', revision: 2 }))
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
    session.begin()
    await flush()
    session.dispose()
    assert.equal(session.operation?.challenge, null)
    assert.equal(session.operation?.terminal, null)
    const shown: string[] = []
    session.onTerminal((text) => shown.push(text))
    assert.deepEqual(shown, [''])
    const before = hub.calls.length
    mock.timers.tick(10_000)
    await flush()
    assert.equal(hub.calls.length, before)
    assert.ok(!hub.calls.some((c) => (c.body as SetupAction).action === 'cancel'))
  } finally { mock.timers.reset() }
})

test('terminal: só no login com código colado; mostra a apresentação filtrada inteira, envia o código numa linha e para ao vencer', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  let now = Date.UTC(2026, 9, 23, 15, 0)
  try {
    const expiresAt = now + 600_000
    const challenge = { kind: 'paste_code' as const, url: 'https://claude.ai/oauth/authorize', displayDomain: 'claude.ai' }
    let terminal: SetupOperation['terminal'] = null
    let phase: SetupOperation['phase'] = 'awaiting_authorization'
    const hub = fakeHub((kind, body) => {
      if (kind === 'start') return op({ phase, expiresAt, challenge })
      const action = body as SetupAction
      if (action.action === 'open_terminal') terminal = { available: true, cursor: 1, output: 'Aguardando o código de autorização do provedor.\r\n\u001b]52;c;eA==\u0007' }
      if (action.action === 'terminal_input') { phase = 'verifying'; terminal = { available: false, cursor: 2, output: '' } }
      if (action.action === 'read' && terminal && action.afterCursor === terminal.cursor) return op({ phase, expiresAt, challenge: phase === 'verifying' ? null : challenge, revision: 2, terminal: { ...terminal, output: '' } })
      return op({ phase, expiresAt, challenge: phase === 'verifying' ? null : challenge, revision: 2, terminal })
    })
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false, now: () => now })
    session.begin()
    await flush()
    assert.equal(session.canOpenTerminal, true)
    const shown: string[] = []
    session.onTerminal((text) => shown.push(text))
    await session.openTerminal()
    assert.equal(session.terminalOpen, true)
    assert.equal(session.terminalWritable, true)
    assert.equal(shown.at(-1), 'Aguardando o código de autorização do provedor.\r\n')
    mock.timers.tick(700)
    await flush()
    const read = hub.calls.filter((c) => (c.body as SetupAction).action === 'read').at(-1)?.body as { afterCursor?: number }
    assert.equal(read.afterCursor, 1)
    assert.equal(shown.at(-1), 'Aguardando o código de autorização do provedor.\r\n', 'mesma versão não redesenha')
    await session.submitTerminalLine(' cod-123_abc ')
    const input = hub.calls.find((c) => (c.body as SetupAction).action === 'terminal_input')?.body as { data: string } | undefined
    assert.equal(input?.data, Buffer.from('cod-123_abc').toString('base64'))
    assert.equal(session.phase, 'verifying')
    assert.equal(session.terminalWritable, false)
    assert.equal(shown.at(-1), '')
    now = expiresAt + 1
    assert.equal(session.overdue, true)
    assert.equal(session.canOpenTerminal, false)
    const before = hub.calls.length
    await session.submitTerminalLine('outro')
    assert.equal(hub.calls.length, before, 'sem terminal aceitando: nada é enviado')
    session.dispose()
  } finally { mock.timers.reset() }
})

test('código no aparelho (Codex) não abre terminal; Preparar passa a revisão nova ao login; conta de chave pede o formulário', async () => {
  const hub = fakeHub((kind, body) => {
    if (kind === 'start' && (body as StartSetup).action === 'install') return op({ operationId: 'inst', phase: 'connected', revision: 9 })
    return op({ operationId: 'login', phase: 'awaiting_authorization', revision: 9, challenge: { kind: 'device_code', url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' } })
  })
  const session = new SetupSession(ACCOUNT, () => 8, client, hub.api, { installFirst: true })
  session.begin()
  await flush()
  const starts = hub.calls.map((c) => c.body as StartSetup)
  assert.deepEqual(starts.map((s) => [s.action, s.expectedRevision]), [['install', 8], ['login', 9]])
  assert.equal(session.canOpenTerminal, false)
  await session.openTerminal()
  assert.equal(session.terminalOpen, false)
  session.dispose()

  const api = fakeHub(() => new Error('api_form_required: use o formulário seguro de API.'))
  const keyed = new SetupSession(ACCOUNT, () => 1, client, api.api, { installFirst: false })
  keyed.begin()
  await flush()
  assert.equal(keyed.errorKind, 'api_form_required')
  assert.match(errorMessage(keyed.error, 'pt-BR'), /chave de API/)
})

test('prazo vale pelo relógio: Start repetido já vencido não mostra o desafio e a leitura confirma o vencimento', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  const now = Date.UTC(2026, 9, 23, 15, 0)
  try {
    const challenge = { kind: 'device_code' as const, url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' }
    const hub = fakeHub((kind) => kind === 'start'
      ? op({ phase: 'awaiting_authorization', expiresAt: now - 1000, challenge })
      : op({ phase: 'expired', revision: 2, errorCode: 'setup_expired' }))
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false, now: () => now })
    session.begin()
    await flush()
    assert.equal(session.phase, 'awaiting_authorization')
    assert.equal(session.operation?.challenge, null, 'desafio vencido não aparece')
    assert.equal(session.overdue, true)
    assert.equal(session.canOpenTerminal, false)
    mock.timers.tick(1500)
    await flush()
    assert.equal(session.phase, 'expired')
  } finally { mock.timers.reset() }
})

test('prazo vence com o terminal aberto: apresentação, código e terminal somem sem esperar o Hub', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  let now = Date.UTC(2026, 9, 23, 15, 0)
  try {
    const expiresAt = now + 60_000
    const challenge = { kind: 'paste_code' as const, url: 'https://claude.ai/oauth/authorize', displayDomain: 'claude.ai' }
    const hub = fakeHub((kind, body) => {
      if (kind === 'start') return op({ phase: 'awaiting_authorization', expiresAt, challenge })
      const action = body as SetupAction
      const terminal = { available: true, cursor: 1, output: 'Aguardando o código de autorização do provedor.\r\n' }
      return op({ phase: 'awaiting_authorization', expiresAt, challenge, revision: 2, terminal: action.action === 'read' && action.afterCursor === 1 ? { ...terminal, output: '' } : terminal })
    })
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false, now: () => now })
    session.begin()
    await flush()
    const shown: string[] = []
    session.onTerminal((text) => shown.push(text))
    await session.openTerminal()
    assert.equal(shown.at(-1), 'Aguardando o código de autorização do provedor.\r\n')
    now = expiresAt + 10
    mock.timers.tick(60_050)
    await flush()
    assert.equal(session.terminalOpen, false)
    assert.equal(shown.at(-1), '')
    assert.equal(session.operation?.challenge, null)
    assert.equal(session.terminalWritable, false)
    session.dispose()
  } finally { mock.timers.reset() }
})

test('falha de rede ao iniciar repete o mesmo pedido; recomeçar depois de vencer usa pedido novo', async () => {
  let fail = true
  let phase: SetupOperation['phase'] = 'expired'
  const hub = fakeHub((kind) => {
    if (kind === 'start' && fail) { fail = false; return Object.assign(new Error('Sem rede'), { status: 0 }) }
    return op({ phase })
  })
  const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
  session.begin()
  await flush()
  assert.match(errorMessage(session.error, 'pt-BR'), /fora de alcance/)
  session.retryStart()
  await flush()
  const [first, second] = hub.calls.map((c) => (c.body as StartSetup).clientRequestId)
  assert.equal(first, second)
  assert.equal(session.phase, 'expired')
  phase = 'failed'
  session.restart()
  await flush()
  const third = (hub.calls.at(-1)!.body as StartSetup).clientRequestId
  assert.notEqual(third, second)
  assert.equal(session.phase, 'failed')
})

test('operação que sumiu do Hub termina como falha e retomar lê a operação lembrada', async () => {
  const hub = fakeHub(() => new Error('operation_expired: releia a conta e inicie outra operação.'))
  const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
  session.resume({ operationId: 'op-velha', generation: 3, step: 'login' })
  await flush()
  const read = hub.calls[0].body as SetupAction
  assert.equal(read.action, 'read')
  assert.equal(read.operationId, 'op-velha')
  assert.equal(read.generation, 3)
  assert.equal(session.phase, 'failed')
  assert.equal(session.operation?.errorCode, 'operation_expired')
})

for (const restart of [false, true]) {
  test(`leitura retida antes de cancelar não restaura desafio${restart ? ' nem substitui o novo login' : ''}`, async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    const challenge = { kind: 'device_code' as const, url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' }
    let release!: (value: SetupOperation) => void
    let starts = 0
    let reads = 0
    const refs: (OperationRef | null)[] = []
    const first = op({ phase: 'awaiting_authorization', generation: 5, challenge })
    const next = op({ operationId: 'op-2', generation: 6, phase: 'awaiting_authorization', challenge })
    const api: SetupApi = {
      start: async () => ++starts === 1 ? first : next,
      action: async (_client, request) => {
        if (request.action === 'cancel') return { ...first, phase: 'cancelled', challenge: null }
        if (++reads === 1) return new Promise((resolve) => { release = resolve })
        return next
      },
    }
    const session = new SetupSession(ACCOUNT, () => 1, client, api, { installFirst: false, onRef: (ref) => refs.push(ref) })
    try {
      session.begin()
      await flush()
      session.retryReading()
      await flush()
      await session.cancel()
      assert.equal(session.phase, 'cancelled')
      if (restart) {
        session.restart()
        await flush()
        // Uma leitura atual precisa funcionar mesmo com a antiga ainda retida.
        session.retryReading()
        await flush()
        assert.equal(reads, 2)
      }
      release(first) // Mesma revisão da resposta cancelled, bytes produzidos antes dela.
      await flush()
      assert.equal(session.phase, restart ? 'awaiting_authorization' : 'cancelled')
      assert.equal(session.operation?.operationId, restart ? 'op-2' : 'op-1')
      assert.deepEqual(refs.at(-1), restart ? { operationId: 'op-2', generation: 6, step: 'login' } : null)
      if (!restart) assert.equal(session.operation?.challenge, null)
    } finally { session.dispose(); mock.timers.reset() }
  })
}

test('cancelar depois do prazo limpa desafio e terminal imediatamente, antes da resposta e do callback atrasado', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  let now = Date.UTC(2026, 9, 24, 2, 0)
  const expiresAt = now + 400
  const awaiting = op({ phase: 'awaiting_authorization', expiresAt,
    challenge: { kind: 'paste_code', url: 'https://claude.ai/oauth/authorize', displayDomain: 'claude.ai' } })
  let reply!: (value: SetupOperation) => void
  const cancelled = new Promise<SetupOperation>((resolve) => { reply = resolve })
  const api: SetupApi = {
    start: async () => awaiting,
    action: async (_client, request) => request.action === 'cancel' ? cancelled : {
      ...awaiting, terminal: { available: true, cursor: 1, output: 'Aguardando o código.\r\n' },
    },
  }
  const session = new SetupSession(ACCOUNT, () => 1, client, api, { installFirst: false, now: () => now })
  const shown: string[] = []
  session.onTerminal((text) => shown.push(text))
  let pending: Promise<void> | undefined
  try {
    session.begin()
    await flush()
    await session.openTerminal()
    assert.equal(session.terminalOpen, true)
    assert.equal(shown.at(-1), 'Aguardando o código.\r\n')
    // O relógio já venceu, mas o callback de expiresAt+50 ainda não executou.
    now = expiresAt + 5
    pending = session.cancel()
    assert.equal(session.operation?.challenge, null)
    assert.equal(session.operation?.terminal, null)
    assert.equal(session.terminalOpen, false)
    assert.equal(shown.at(-1), '')
    assert.equal(session.phase, 'awaiting_authorization', 'limpar não inventa a confirmação do Hub')
    assert.equal(session.cancelling, true)
    assert.equal(session.terminalWritable, false)
    await flush()
    assert.equal(session.operation?.challenge, null, 'continua limpo com o cancel retido')
  } finally {
    reply({ ...awaiting, phase: 'expired', challenge: null, terminal: null })
    await pending
    session.dispose()
    mock.timers.reset()
  }
  assert.equal(session.phase, 'expired')
})


test('perda da capacidade remota apaga desafio imediatamente sem repetir login', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    const challenge = { kind: 'device_code' as const, url: 'https://auth.openai.com/codex/device', displayDomain: 'auth.openai.com', code: 'ABCD-1234' }
    const hub = fakeHub((kind) => kind === 'start' ? op({ phase: 'awaiting_authorization', challenge })
      : new Error('remote_setup_unavailable: autorização expirada.'))
    const session = new SetupSession(ACCOUNT, () => 1, client, hub.api, { installFirst: false })
    session.begin()
    await flush()
    assert.equal(session.operation?.challenge?.code, 'ABCD-1234')
    mock.timers.tick(2000)
    await flush()
    assert.equal(session.phase, 'failed')
    assert.equal(session.operation?.challenge, null)
    assert.equal(session.operation?.terminal, null)
    mock.timers.tick(60_000)
    await flush()
    assert.equal(hub.calls.filter((c) => c.kind === 'start').length, 1)
    assert.equal(hub.calls.filter((c) => c.kind === 'action').length, 1)
    session.dispose()
  } finally { mock.timers.reset() }
})
