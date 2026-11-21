import assert from 'node:assert/strict'
import test from 'node:test'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { accountKind, accountView, botNames, createTerminalFilter, errorMessage, failureMessage, expiresAtMs, isFinal, keyPageUrl, providerUrl, rowActions, stepOf, testResult, utf8ToBase64, validLabel, viaProgramaOficial } from './regras.ts'

const account = (patch: Partial<ManagedAccount> = {}): ManagedAccount => ({
  id: `ac_${'a'.repeat(64)}`, label: 'Pessoal', brand: 'OpenAI', backend: 'codex', billing: 'unverified', state: 'disconnected',
  models: [], revision: 1, origin: 'managed', botIds: [], ...patch,
})

test('só abre https no domínio exato da lista do provedor, sem usuário, senha ou porta', () => {
  assert.equal(providerUrl('codex', 'https://auth.openai.com/codex/device', 'auth.openai.com'), 'https://auth.openai.com/codex/device')
  assert.equal(providerUrl('claude', 'https://claude.ai/oauth/authorize?code=true', 'claude.ai'), 'https://claude.ai/oauth/authorize?code=true')
  for (const raw of [
    'http://auth.openai.com/codex/device',
    'https://auth.openai.com.evil.example/codex/device',
    'https://evil.example/?u=https://auth.openai.com',
    'https://user:pass@auth.openai.com/codex/device',
    'https://auth.openai.com:8443/codex/device',
    'https://аuth.openai.com/codex/device',
    'https://auth.openai.com./codex/device',
    'javascript:alert(1)',
    'https://auth.openai.com/codex/device\nX-Header: 1',
    ' https://auth.openai.com/codex/device',
    'https://claude.ai/oauth/authorize',
  ]) assert.equal(providerUrl('codex', raw, 'auth.openai.com'), null, raw)
  // O domínio exibido precisa ser o mesmo da URL; outro domínio oficial não serve de disfarce.
  assert.equal(providerUrl('claude', 'https://claude.ai/oauth/authorize', 'console.anthropic.com'), null)
  assert.equal(providerUrl('codex', 42), null)
  assert.equal(providerUrl('codex', `https://auth.openai.com/${'a'.repeat(2100)}`), null)
  assert.equal(keyPageUrl('codex', 'https://platform.openai.com/api-keys'), 'https://platform.openai.com/api-keys')
  assert.equal(keyPageUrl('codex', 'https://auth.openai.com/api-keys'), null)
})

test('o filtro do terminal tira OSC (área de transferência, links, título), DCS/APC e C1, e mantém texto e cores', () => {
  const filter = createTerminalFilter()
  const out = filter.push([
    'Abra \u001b[1mhttps://auth.openai.com/codex/device\u001b[0m\r\n',
    '\u001b]52;c;Y29waWFy\u0007',
    '\u001b]8;;https://evil.example\u001b\\clique\u001b]8;;\u001b\\',
    '\u001b]0;título\u0007',
    '\u001bPq$x\u001b\\',
    '\u001b_apc\u001b\\',
    '\u009d52;c;eA==\u009c',
    'fim\u0007\n',
  ].join(''))
  assert.equal(out, 'Abra \u001b[1mhttps://auth.openai.com/codex/device\u001b[0m\r\ncliquefim\n')
  assert.ok(!out.includes(']52'))
})

test('o filtro descarta pedidos que fariam o xterm responder ou capturar mouse, foco e colagem', () => {
  const filter = createTerminalFilter()
  const out = filter.push('a\u001b[6nb\u001b[cc\u001b[?1000hd\u001b[?2004he\u001b[?1004hf\u001b[>0cg\u001b[14th\u001b[?25li\u001b[2Kj\u001b[1;31mk\u001b[ql')
  assert.equal(out, 'ab' + 'c' + 'd' + 'e' + 'f' + 'g' + 'h\u001b[?25li\u001b[2Kj\u001b[1;31mk' + 'l')
})

test('sequência partida entre pedaços continua filtrada, sem vazar o conteúdo', () => {
  const filter = createTerminalFilter()
  const parts = ['ok \u001b', ']52;c;', 'Y29w', 'aWFy\u001b', '\\ depois \u001b[3', '1mcor\u001b[0m']
  assert.equal(parts.map((p) => filter.push(p)).join(''), 'ok  depois \u001b[31mcor\u001b[0m')
  const c1 = createTerminalFilter()
  assert.equal(c1.push('x\u009d0;t') + c1.push('itulo\u0007y'), 'xy')
})

test('OSC longo demais é descartado até o terminador, sem guardar o conteúdo', () => {
  const filter = createTerminalFilter(16)
  assert.equal(filter.push('a\u001b]52;c;' + 'A'.repeat(64)), 'a')
  assert.equal(filter.push('B'.repeat(64)), '')
  assert.equal(filter.push('C\u001b'), '')
  assert.equal(filter.push('\\depois'), 'depois')
  filter.reset()
  assert.equal(filter.push('\u001b'), '')
  assert.equal(filter.push('[32mverde'), '\u001b[32mverde')
})

test('erros do Hub viram texto em português; desconhecido usa a mensagem do Hub; queda de rede é falta de alcance', () => {
  assert.match(errorMessage(new Error('remote_setup_unavailable: setup remoto indisponível.'), 'pt-BR'), /autorização deste aparelho não está vigente/)
  assert.match(errorMessage(new Error('account_conflict: releia a conta antes de continuar.'), 'pt-BR'), /mudou em outra janela/)
  assert.equal(errorMessage(new Error('label_taken: já existe uma conta com este nome.'), 'pt-BR'), 'já existe uma conta com este nome.')
  assert.match(errorMessage({ status: 0, message: "Can't reach your computer." }, 'pt-BR'), /fora de alcance/)
  assert.match(errorMessage(new Error('x'.repeat(300)), 'pt-BR'), /Nada foi alterado/)
})

test('estado e ação de cada conta: conectar, reconectar, chave, atualizar; perfil antigo não é gerenciado', () => {
  assert.deepEqual(accountView(account(), 'cli', 'pt-BR').primary, { action: 'connect', label: 'Conectar' })
  assert.deepEqual(accountView(account({ state: 'unverified', billing: 'subscription' }), 'cli', 'pt-BR').primary, { action: 'reconnect', label: 'Reconectar' })
  // Conta nova: o Hub cria como não verificada e só sonda depois do primeiro login.
  const fresh = accountView(account({ state: 'unverified' }), 'cli', 'pt-BR')
  assert.equal(fresh.state, 'Não conectada')
  assert.equal(fresh.billing, '')
  assert.deepEqual(fresh.primary, { action: 'connect', label: 'Conectar' })
  assert.deepEqual(accountView(account({ state: 'unverified' }), 'api', 'pt-BR').primary, { action: 'key', label: 'Informar chave' })
  assert.deepEqual(accountView(account(), 'api', 'pt-BR').primary, { action: 'key', label: 'Informar chave' })
  assert.deepEqual(accountView(account({ state: 'unavailable' }), 'cli', 'pt-BR').primary, { action: 'refresh', label: 'Tentar de novo' })
  const connected = accountView(account({ state: 'connected', billing: 'subscription' }), 'cli', 'pt-BR')
  assert.equal(connected.primary, null)
  assert.equal(connected.billing, 'Assinatura confirmada')
  const legacy = accountView(account({ origin: 'legacy', label: 'Codex — perfil do Hub' }), 'cli', 'pt-BR')
  assert.equal(legacy.title, 'Perfil antigo do Hub · Codex')
  assert.equal(legacy.primary, null)
  assert.equal(legacy.managed, false)
  // Sem tipo conhecido, só API paga indica chave; o tipo criado nesta sessão prevalece.
  assert.equal(accountKind(account({ billing: 'paid_api' })), 'api')
  assert.equal(accountKind(account()), 'cli')
  assert.equal(accountKind(account(), 'api'), 'api')
})

test('nomes dos bots, rótulo válido, fases e prazos', () => {
  const names: Record<string, string> = { a: 'Donna', b: 'Byte', c: 'Sprint', d: 'Nina' }
  const name = (id: string) => names[id] ?? null
  // DON-917: a lista vem do Intl.ListFormat do idioma, e cada nome vai isolado (FSI…PDI) com a direção dele.
  const plano = (texto: string) => texto.replace(/[\u2068\u2069]/g, '')
  assert.equal(botNames([], name, 'pt-BR'), '')
  assert.equal(plano(botNames(['a'], name, 'pt-BR')), 'Donna')
  assert.equal(botNames(['a'], name, 'pt-BR'), '\u2068Donna\u2069')
  assert.equal(plano(botNames(['a', 'b'], name, 'pt-BR')), 'Donna e Byte')
  assert.equal(plano(botNames(['a', 'b', 'c', 'd'], name, 'pt-BR')), 'Donna, Byte, Sprint e mais 1')
  assert.equal(botNames(['x', 'y'], name, 'pt-BR'), '2 bots')
  assert.equal(plano(botNames(['a', 'b', 'c', 'd'], name, 'en')), 'Donna, Byte, Sprint, and 1 more')
  assert.equal(botNames(['x', 'y'], name, 'ar-AE'), 'بوتان')
  assert.ok(validLabel(' Pessoal '))
  assert.ok(!validLabel('   '))
  assert.ok(!validLabel('a'.repeat(61)))
  assert.ok(!validLabel('linha\nquebrada'))
  assert.deepEqual(['preparing', 'awaiting_authorization', 'verifying', 'connected', 'cancelled'].map((p) => stepOf(p as never)), [0, 1, 2, 3, null])
  assert.deepEqual(['connected', 'cancelled', 'expired', 'failed', 'verifying'].map((p) => isFinal(p as never)), [true, true, true, true, false])
  assert.equal(expiresAtMs(1_800_000_000), 1_800_000_000_000)
  assert.equal(expiresAtMs(1_800_000_000_000), 1_800_000_000_000)
  assert.equal(utf8ToBase64('código\r'), Buffer.from('código\r', 'utf8').toString('base64'))
})

test('DON-917: o mesmo erro cru do Hub sai no idioma de agora; código desconhecido usa a mensagem do Hub', () => {
  const cru = new Error('account_busy: um bot está trabalhando.')
  assert.match(errorMessage(cru, 'pt-BR'), /Espere o turno terminar/)
  assert.match(errorMessage(cru, 'en'), /Wait for its turn to finish/)
  assert.match(errorMessage({ status: 0 }, 'ar-AE'), /خارج النطاق/)
  assert.equal(errorMessage(new Error('label_taken: nome repetido.'), 'en'), 'nome repetido.')
  assert.match(failureMessage('identity_changed', 'fr'), /autre compte/)
  assert.equal(failureMessage('codigo_novo', 'en'), "Couldn’t connect.")
})

test('DON-917 (checklist 6): nome do Hub ou da conta isolado na frase; domínio oficial sempre LTR', async () => {
  const { textosEm } = await import('./textos.ts')
  const ar = textosEm('ar-AE')
  assert.ok(ar.removerTitulo('MacBook Pro').includes('⁨MacBook Pro⁩'))
  assert.ok(ar.paginaOficial('claude.ai').includes('⁦claude.ai⁩'))
  assert.ok(textosEm('en').removerTitulo('Trabalho').includes('⁨Trabalho⁩'))
})

test('ações à vista: Conectada testa/exclui; Desconectada reconecta/exclui; perfil antigo só testa, ou reconecta (conta nova) e sai se desconectado', () => {
  const ver = (patch: Partial<ManagedAccount>) => { const a = account(patch); return rowActions(a, accountKind(a), 'pt-BR') }
  const conectada = ver({ state: 'connected', billing: 'subscription' })
  assert.deepEqual([conectada.test, conectada.connect, conectada.remove], [true, null, 'free'])
  const desconectada = ver({ state: 'disconnected' })
  assert.deepEqual([desconectada.test, desconectada.connect, desconectada.remove], [false, { action: 'connect', label: 'Reconectar' }, 'free'])
  const semLogin = ver({ state: 'unverified', billing: 'unverified' })
  assert.deepEqual([semLogin.test, semLogin.connect?.label, semLogin.remove], [false, 'Conectar', 'free'])
  const naoConfirmada = ver({ state: 'unverified', billing: 'subscription' })
  assert.deepEqual([naoConfirmada.test, naoConfirmada.connect?.action, naoConfirmada.remove], [true, 'reconnect', 'free'])
  const comProblema = ver({ state: 'unavailable' })
  assert.deepEqual([comProblema.test, comProblema.connect?.action], [true, 'reconnect'])
  const porChave = ver({ state: 'disconnected', billing: 'paid_api' })
  assert.equal(porChave.connect?.action, 'key')
  // Com bots usando a conta: a tela explica em vez de pedir o que o Hub recusaria.
  assert.equal(ver({ state: 'connected', botIds: ['berga'] }).remove, 'in-use')
  // Perfil antigo: conectado é o login do computador (sem Excluir); desconectado entra por conta nova e pode sair da lista.
  const antigaConectada = ver({ origin: 'legacy', state: 'connected', billing: 'subscription' })
  assert.deepEqual([antigaConectada.test, antigaConectada.connect, antigaConectada.remove], [true, null, null])
  const antigaDesconectada = ver({ origin: 'legacy', state: 'disconnected' })
  assert.deepEqual([antigaDesconectada.test, antigaDesconectada.connect, antigaDesconectada.remove], [false, { action: 'new', label: 'Reconectar' }, 'free'])
})

test('resultado do Testar: conectada passa sem custo; senão, falha com o motivo curto do estado', () => {
  const ok = testResult(account({ state: 'connected', billing: 'subscription' }), 'pt-BR')
  assert.equal(ok.ok, true)
  assert.match(ok.text, /Nada foi enviado/)
  const falha = testResult(account({ state: 'unavailable' }), 'pt-BR')
  assert.equal(falha.ok, false)
  assert.match(falha.text, /^Falhou: /)
  assert.equal(testResult(account({ state: 'connected' }), 'en').text, 'Working. Nothing was sent and it cost nothing.')
})

test('a etiqueta do programa oficial só aparece na conta Claude por assinatura', () => {
  const claude = account({ backend: 'claude', brand: 'Anthropic', billing: 'subscription' })
  assert.equal(viaProgramaOficial(claude, accountKind(claude)), true)
  assert.equal(viaProgramaOficial(account({ backend: 'claude', billing: 'unverified' }), 'cli'), true)
  assert.equal(viaProgramaOficial(account({ backend: 'claude', billing: 'paid_api' }), 'api'), false)
  assert.equal(viaProgramaOficial(account({ backend: 'codex', billing: 'subscription' }), 'cli'), false)
})
