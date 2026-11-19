import assert from 'node:assert/strict'
import test from 'node:test'
import { spokenText } from '../../call/spoken-text.ts'
import { CallOperator } from '../../call/call-operator.ts'
import { pediuEncerramento, pediuSilencio } from './encerrar.ts'
import { ENVIO_FALHOU, HUB_DESCONECTADO } from './operador-voz.ts'

// D416 A6: o texto falado perde o que só serve na tela; a conclusão, nomes e datas ficam.

test('código inline técnico sai; palavra simples entre crases fica', () => {
  assert.equal(spokenText('Rodei `npm run typecheck` e passou.'), 'Rodei e passou.')
  assert.equal(spokenText('A função `pediuEncerramento()` mudou.'), 'A função mudou.')
  assert.equal(spokenText('Mesclei na `main`.'), 'Mesclei na main.')
  assert.equal(spokenText('Antes:\n```ts\nconst x = 1\n```\nPronto.'), 'Antes:\nPronto.')
})

test('URLs soltas, autolinks e links Markdown saem; o texto do link fica', () => {
  assert.equal(spokenText('Abri o PR https://github.com/acme/repo/pull/12, pode revisar.'), 'Abri o PR, pode revisar.')
  assert.equal(spokenText('Veja <https://exemplo.com/a?b=1>.'), 'Veja.')
  assert.equal(spokenText('Veja www.exemplo.com/docs agora.'), 'Veja agora.')
  assert.equal(spokenText('Li [o guia de voz](https://exemplo.com/voz) inteiro.'), 'Li o guia de voz inteiro.')
  assert.equal(spokenText('Link: [https://x.dev/a](https://x.dev/a)'), 'Link:')
})

test('caminhos Unix, relativos e Windows saem; datas e barras comuns ficam', () => {
  assert.equal(spokenText('Salvei o relatório em /Users/ana/Documents/relatorio.md para você.'), 'Salvei o relatório em para você.')
  assert.equal(spokenText('Mudei ~/projetos/donna/config.toml e ./src/app.ts hoje.'), 'Mudei e hoje.')
  assert.equal(spokenText('O teste em apps/desktop/src/x.test.ts falhou.'), 'O teste em falhou.')
  assert.equal(spokenText('Editei docs/voice-call.md.'), 'Editei.')
  assert.equal(spokenText('Ficou em C:\\Users\\Ana\\Desktop\\nota.txt agora.'), 'Ficou em agora.')
  assert.equal(spokenText('Reunião em 07/11/2026, 24/7 e/ou a 80 km/h.'), 'Reunião em 07/11/2026, 24/7 e/ou a 80 km/h.')
})

test('hashes, UUIDs, IDs com prefixo e tokens longos saem; números e nomes ficam', () => {
  assert.equal(spokenText('Commit a1b2c3d4e5 enviado.'), 'Commit enviado.')
  assert.equal(spokenText('Sessão 3f2504e0-4f89-11d3-9a0c-0305e82c3301 aberta.'), 'Sessão aberta.')
  assert.equal(spokenText('Thread thr_abc123 terminou.'), 'Thread terminou.')
  assert.equal(spokenText('Token eyJhbGciOiJIUzI1NiIsInR5cCI6 recusado.'), 'Token recusado.')
  assert.equal(spokenText('O PR #462 da Donna passou em 1234567 casos, numa fachada decade.'), 'O PR #462 da Donna passou em 1234567 casos, numa fachada decade.')
  assert.equal(spokenText('Covid-19 e GPT-6 são nomes.'), 'Covid-19 e GPT-6 são nomes.')
})

// D416-R3/R4: caminhos de um componente e com acento saem inteiros; preço, agenda e datas por extenso ficam.
test('caminhos curtos e com acento saem inteiros; preço e agenda com barras ficam', () => {
  assert.equal(spokenText('Salvei em ~/Downloads.'), 'Salvei em.')
  assert.equal(spokenText('Use /tmp para isso.'), 'Use para isso.')
  assert.equal(spokenText('Salvei em /Users/Ana/relatório.pdf.'), 'Salvei em.')
  assert.equal(spokenText('O plano custa 20 USD/user/month.'), 'O plano custa 20 USD/user/month.')
  assert.equal(spokenText('As reuniões são seg/qua/sex.'), 'As reuniões são seg/qua/sex.')
  assert.equal(spokenText('Vence em Nov/07/2026.'), 'Vence em Nov/07/2026.')
  assert.equal(spokenText('Editei src/renderer/views/Ação.tsx hoje.'), 'Editei hoje.')
})

// D416-R2: parêntese não fechado com muitos espaços não pode custar tempo quadrático.
for (const [nome, texto] of [
  ['parêntese aberto com espaços', `Veja (${' '.repeat(40000)}X`],
  ['token com hífens', `ID ${'a-'.repeat(20000)}1`],
  ['palavra longa', `Veja ${'a'.repeat(40000)}.`],
  ['barras', `Veja ${'a/'.repeat(20000)}`],
  ['pontos', `Veja ${'.'.repeat(40000)}/x`],
  ['caminho longo', `Veja /${'a/'.repeat(20000)}`],
]) test(`formatador linear: ${nome}`, () => {
  const inicio = performance.now()
  spokenText(texto)
  assert.ok(performance.now() - inicio < 250, `${Math.round(performance.now() - inicio)} ms`)
})

test('horário perde só os segundos; data fica', () => {
  assert.equal(spokenText('Terminou às 14:32:05.'), 'Terminou às 14:32.')
  assert.equal(spokenText('Rodou em 2026-11-07T14:32:05.123Z.'), 'Rodou em 2026-11-07 14:32.')
  assert.equal(spokenText('Reunião às 9:30 de 07/11.'), 'Reunião às 9:30 de 07/11.')
})

test('conclusão sobrevive quando só há detalhe técnico ao redor', () => {
  assert.equal(spokenText('Pronto: os testes passaram. Detalhes em `/tmp/log.txt` (sha 9f8e7d6c5b).'), 'Pronto: os testes passaram. Detalhes em.')
})

// Consumidores reais: só a cópia que vai à voz muda; a entrada do chat fica intacta.
const textoChat = 'Abri https://github.com/acme/repo/pull/12 com `git push` às 14:32:05.'
function operador() {
  const entry = { id: 'e1', kind: 'agent', rev: 2, data: { text: textoChat, final: true } }
  const store = {
    bots: new Map([['bot', { id: 'bot', name: 'Donna', status: 'idle' }]]),
    members: () => [],
    send: () => {},
    respond: () => {},
    allEntries: () => [entry],
  } as unknown as ConstructorParameters<typeof CallOperator>[0]
  return { op: new CallOperator(store, 'bot'), entry }
}

test('reply, recent_messages e notice usam o formatador sem mutar o chat', () => {
  const { op, entry } = operador()
  assert.equal(op.reply(textoChat), '[Donna replied] Abri com às 14:32.')
  assert.equal(op.notice('Donna precisa da sua aprovação em https://x.dev/c/1.'), '[Notice] Donna precisa da sua aprovação em.')
  assert.deepEqual(JSON.parse(op.call('recent_messages', { count: 1 })), { messages: [{ from: 'Donna', text: 'Abri com às 14:32.' }] })
  assert.equal(entry.data.text, textoChat)
})

// D416 A5: despedidas encerram; para/espera/fica quieto não.
test('despedidas simples encerram em pt-BR e inglês', () => {
  for (const t of ['tchau', 'Tchau!', 'tchau tchau', 'Até depois.', 'até mais', 'Até logo!', 'ok, tchau', 'Valeu, tchau!', 'Obrigada, até depois.', 'Tchau, pode fechar!', 'desliga']) assert.equal(pediuEncerramento(t, 'pt-BR'), true, t)
  for (const t of ['bye', 'Goodbye!', 'bye bye', 'Talk to you later.', 'see you later', 'Thanks, bye!', 'ok bye', 'hang up']) assert.equal(pediuEncerramento(t, 'en-US'), true, t)
})

test('citação, negação, hipótese, outro objeto e agradecimento sozinho não encerram', () => {
  for (const t of ['ela disse "tchau"', '“tchau”', 'não tchau', 'se eu disser tchau desliga', 'tchau pro servidor', 'manda tchau pra Ana', 'até depois de amanhã', 'obrigado', 'ok', 'valeu', 'para', 'espera', 'fica quieto']) assert.equal(pediuEncerramento(t, 'pt-BR'), false, t)
  for (const t of ['he said "bye"', "don't say bye", 'say bye to Ana', 'thanks', 'ok', 'stop', 'wait', 'be quiet']) assert.equal(pediuEncerramento(t, 'en-US'), false, t)
  assert.equal(pediuEncerramento('tchau', 'en-US'), false)
  assert.equal(pediuEncerramento('bye', 'pt-BR'), false)
  assert.equal(pediuEncerramento('tchau', 'fr-FR'), false)
})

test('para/espera/fica quieto pedem silêncio, não encerramento; frases maiores seguem para o bot', () => {
  for (const t of ['para', 'Pare!', 'espera', 'Espera aí.', 'pera', 'fica quieto', 'Fica quieta, por favor.', 'silêncio', 'para de falar']) assert.equal(pediuSilencio(t, 'pt-BR'), true, t)
  for (const t of ['stop', 'Wait!', 'hold on', 'be quiet', 'stop talking, please']) assert.equal(pediuSilencio(t, 'en-US'), true, t)
  for (const t of ['para o servidor', 'espera o deploy terminar e me avisa', 'não para', 'ela disse "para"', 'tchau', 'para tudo e apaga o banco']) assert.equal(pediuSilencio(t, 'pt-BR'), false, t)
  for (const t of ['stop the server', "don't stop", 'bye']) assert.equal(pediuSilencio(t, 'en-US'), false, t)
  assert.equal(pediuSilencio('para', 'en-US'), false)
})

// D416 A1/A3/A6: orientação compacta ao modelo; regra instruída, não comportamento provado.
test('instruções da sessão pedem uma confirmação curta, silêncio e só a conclusão em voz', () => {
  const { op } = operador()
  const instr = op.instructions
  assert.match(instr, /sent: true.*uma vez.*silêncio/s)
  assert.match(instr, /"tchau".*"até depois"/s)
  assert.match(instr, /para.*espera.*fica quieto.*continue ouvindo/is)
  assert.match(instr, /chat/)
  assert.match(instr, /o que falhou.*próximo passo/s)
})

test('erros falados dizem o que falhou, o impacto e o próximo passo, sem código', () => {
  for (const t of [HUB_DESCONECTADO, ENVIO_FALHOU]) {
    assert.ok(t.split(/(?<=\.)\s/).length >= 2, `falha + próximo passo: ${t}`)
    assert.doesNotMatch(t, /\b\d{3}\b|Error|stack|https?:|`/)
    assert.equal(spokenText(t), t)
  }
})
