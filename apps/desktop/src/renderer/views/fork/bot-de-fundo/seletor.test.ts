import assert from 'node:assert/strict'
import { test } from 'node:test'
import { intencaoDeTroca, modeloNaConta, type Provedor } from '../seletor-modelo/regras.ts'
import { escolhaDoRascunho, fraseDaFuncao, motivoDaConta, rascunhoDaEscolha, resumoDaEscolha, rotulosUnicos, SEM_ESCOLHA, seletorDaFuncao, type TextosSeletor } from './seletor.ts'
import { MEMORIA_OCIOSA_MIN, type Account, type Draft, type FunctionSnapshot } from './model.ts'

const conta = (id: string, extra: Partial<Account> = {}): Account => ({ id, label: `Conta ${id}`, backend: 'claude', state: 'connected', billing: 'subscription', models: [{ id: 'haiku', options: [] }, { id: 'sonnet', options: [] }], ...extra })
const dados = (accounts: Account[]): FunctionSnapshot => ({ document: null, accounts, accountsIssue: null, functions: [], execution: '' })
const rascunho = (extra: Partial<Draft> = {}): Draft => ({ functionId: 'memory_consolidation', baseRevision: 1, mode: 'automatic', accountId: '', modelId: '', modelOptions: {}, alternativeAccountId: '', ...extra })
const catalogo: Provedor[] = [{ id: 'claude', name: 'Claude', models: [{ id: 'haiku', name: 'Haiku' }, { id: 'sonnet', name: 'Sonnet' }] }, { id: 'codex', name: 'Codex', models: [{ id: 'gpt', name: 'GPT' }] }]
const abrir = () => undefined
const textos: TextosSeletor = {
  legacy: 'Perfil antigo',
  estado: { desconectada: 'desconectada', indisponivel: 'indisponível', naoConfirmada: 'login não confirmado', motorRecusado: 'x', apiEmBreve: 'api em breve', semAssinatura: 'sem assinatura', semModelo: 'sem modelo', conectada: 'conectada' },
  conectar: 'Conectar', confirmarLogin: 'Confirmar login',
  automatico: { nome: 'Automático · leve', nomeCodex: 'Automático · Codex', etiqueta: 'Recomendado', meta: 'hoje: Haiku' },
  nota: 'Vale a partir da próxima rodada.', recusadoCurto: 'Só roda no Claude', recusado: (m) => `${m} não serve`, semConta: (m) => `sem ${m}`, trocarDica: (c) => `usar ${c}`, automaticoFora: (c) => `Automático não funciona em ${c}`,
}
const monta = (accounts: Account[], draft: Draft, o: { functionId?: string; desconectada?: boolean } = {}) =>
  seletorDaFuncao({ data: dados(accounts), functionId: o.functionId ?? 'memory_consolidation', draft: { ...draft, functionId: o.functionId ?? draft.functionId }, catalogo, desconectada: o.desconectada ?? false, textos, abrirContas: abrir })

test('conta conectada com modelos serve e vira escolha; Automático é modelo vazio', () => {
  const s = monta([conta('a')], rascunho({ accountId: 'a' }))
  assert.deepEqual(s.contas.map((c) => c.id), ['a'])
  assert.deepEqual(s.valor, { accountId: 'a', backend: 'claude', model: null, modelOptions: {}, fastMode: false })
  assert.deepEqual(s.funcao.automatico, { backend: 'claude', nome: 'Automático · leve', etiqueta: 'Recomendado', meta: 'hoje: Haiku' })
  assert.deepEqual(resumoDaEscolha(s, textos), { motor: 'Claude', conta: 'Conta a', modelo: 'Automático · leve' })
})

test('seleção válida com modelo à mão: conta, modelo e esforço voltam ao rascunho sem perder nada', () => {
  const comEsforco = conta('a', { models: [{ id: 'haiku', options: [] }, { id: 'sonnet', options: [{ id: 'effort', values: ['low', 'high'], default: null }] }] })
  const s = monta([comEsforco], rascunho({ mode: 'custom', accountId: 'a', modelId: 'sonnet', modelOptions: { effort: 'low' } }))
  assert.deepEqual(s.valor, { accountId: 'a', backend: 'claude', model: 'sonnet', modelOptions: { effort: 'low' }, fastMode: false })
  assert.equal(resumoDaEscolha(s, textos)?.modelo, 'Sonnet')
  assert.deepEqual(rascunhoDaEscolha(s.valor), { mode: 'custom', accountId: 'a', modelId: 'sonnet', modelOptions: { effort: 'low' } })
  assert.deepEqual(rascunhoDaEscolha({ ...s.valor, model: null, modelOptions: {} }), { mode: 'automatic', accountId: 'a', modelId: '', modelOptions: {} })
})

test('conta desconectada nunca vem escolhida e nunca é marcável; aparece com o estado e a ação Conectar', () => {
  const contas = [conta('ok'), conta('caiu', { state: 'disconnected' })]
  const s = monta(contas, rascunho({ accountId: 'caiu' }), { desconectada: true })
  assert.deepEqual(s.valor, SEM_ESCOLHA)
  assert.equal(resumoDaEscolha(s, textos), null)
  assert.deepEqual(s.contas.map((c) => c.id), ['ok'], 'só a pronta é marcável')
  assert.deepEqual(s.funcao.inativas.map((c) => [c.id, c.estado, c.acao?.rotulo]), [['caiu', 'desconectada', 'Conectar']])
  // Mesmo sem o aviso da resolução, uma conta fora do conjunto que serve não vira escolha.
  assert.deepEqual(escolhaDoRascunho(rascunho({ accountId: 'caiu' }), s.contas, contas), SEM_ESCOLHA)
})

test('login não confirmado oferece Confirmar login; indisponível e sem modelo não têm ação', () => {
  const s = monta([conta('u', { state: 'unverified' }), conta('i', { state: 'unavailable' }), conta('m', { models: [] })], rascunho())
  assert.equal(s.contas.length, 0, 'nenhuma pronta')
  assert.deepEqual(s.funcao.inativas.map((c) => [c.id, c.estado, c.acao?.rotulo]), [['u', 'login não confirmado', 'Confirmar login'], ['i', 'indisponível', undefined], ['m', 'sem modelo', undefined]])
  assert.deepEqual(s.valor, SEM_ESCOLHA)
})

test('função que não aceita motor: Codex fica recusado com o motivo; as contas dele não são marcáveis', () => {
  const contas = [conta('a'), conta('x', { backend: 'codex', models: [{ id: 'gpt', options: [] }] })]
  const s = monta(contas, rascunho({ accountId: 'x' }))
  assert.deepEqual(Object.keys(s.funcao.recusados), ['codex'])
  assert.equal(s.funcao.recusados.codex!.motivo, 'Codex não serve')
  assert.deepEqual(s.contas.map((c) => c.id), ['a'])
  assert.deepEqual(s.funcao.inativas.map((c) => [c.id, c.estado, c.acao]), [['x', 'conectada', undefined]])
  assert.deepEqual(s.valor, SEM_ESCOLHA, 'escolha salva num motor recusado não vem marcada')
  // Função que aceita Codex (visão): nada recusado e o Automático é do Codex.
  const v = monta(contas, rascunho({ accountId: 'x' }), { functionId: 'vision' })
  assert.deepEqual(v.funcao.recusados, {})
  assert.equal(v.funcao.automatico, null, 'o Hub não resolve o padrão do Codex: sem Automático, nem recomendado')
  assert.deepEqual(v.contas.map((c) => c.id), ['a', 'x'])
})

test('memória e títulos só aceitam assinatura confirmada; chave de API e assinatura não confirmada ficam de fora com o motivo', () => {
  assert.equal(motivoDaConta(conta('a'), 'memory_consolidation'), null)
  assert.equal(motivoDaConta(conta('a', { billing: 'paid_api' }), 'bot_naming'), 'apiEmBreve')
  assert.equal(motivoDaConta(conta('a', { billing: 'unverified' }), 'memory_consolidation'), 'semAssinatura')
})

test('P2-3: API e cobrança desconhecida são recusadas em TODAS as funções, como `binding_capabilities` do Hub', () => {
  for (const fn of ['memory_consolidation', 'bot_naming', 'vision', 'context_compression', 'bot_draft', 'skill_selection']) {
    assert.equal(motivoDaConta(conta('a', { billing: 'paid_api' }), fn), 'apiEmBreve', fn)
    assert.equal(motivoDaConta(conta('a', { billing: 'api' }), fn), 'apiEmBreve', fn)
    assert.equal(motivoDaConta(conta('a', { billing: 'unknown' }), fn), 'semAssinatura', fn)
    assert.equal(motivoDaConta(conta('a', { billing: '' }), fn), 'semAssinatura', fn)
    assert.equal(motivoDaConta(conta('a', { billing: 'tipo-novo' }), fn), 'semAssinatura', `${fn}: cobrança que o app não conhece = recusa`)
    assert.equal(motivoDaConta(conta('a'), fn), null, fn)
  }
  assert.equal(motivoDaConta(conta('a', { state: 'estado-novo' }), 'vision'), 'indisponivel', 'estado desconhecido = o Hub trata como desconectada: recusa')
  assert.equal(motivoDaConta(conta('a', { state: 'unverified' }), 'vision'), 'naoConfirmada')
  // Em Visão, conta de API ou de cobrança desconhecida nunca é marcável nem vem escolhida; fica na linha Conta com o motivo.
  for (const [billing, estado] of [['paid_api', 'api em breve'], ['unknown', 'sem assinatura']] as const) {
    const contas = [conta('ok', { backend: 'codex', models: [{ id: 'gpt', options: [] }] }), conta('x', { backend: 'codex', billing, models: [{ id: 'gpt', options: [] }] })]
    const s = monta(contas, rascunho({ mode: 'custom', accountId: 'x', modelId: 'gpt' }), { functionId: 'vision' })
    assert.deepEqual(s.contas.map((c) => c.id), ['ok'])
    assert.deepEqual(s.funcao.inativas.map((c) => [c.id, c.estado]), [['x', estado]])
    assert.deepEqual(s.valor, SEM_ESCOLHA, `${billing}: a escolha salva não vem marcada`)
    assert.deepEqual(s.semEscolha, { chave: 'fundo.seletor.salvaConta', params: { conta: 'Conta x', estado } })
  }
})

test('P2-1: modelo salvo que a conta não tem mais não vem escolhido, mesmo que o catálogo do chat ainda o liste; o rascunho fica e o motivo aparece', () => {
  const comOpcao = conta('a', { models: [{ id: 'haiku', options: [] }, { id: 'sonnet', options: [{ id: 'effort', values: ['low', 'medium'], default: null }] }] })
  const salvo = rascunho({ mode: 'custom', accountId: 'a', modelId: 'opus-removido', modelOptions: {} })
  const antes = structuredClone(salvo)
  const noChat: Provedor[] = [{ id: 'claude', name: 'Claude', models: [{ id: 'haiku', name: 'Haiku' }, { id: 'sonnet', name: 'Sonnet' }, { id: 'opus-removido', name: 'Opus' }] }]
  const s = seletorDaFuncao({ data: dados([comOpcao]), functionId: 'memory_consolidation', draft: salvo, catalogo: noChat, desconectada: false, textos, abrirContas: abrir })
  assert.deepEqual(s.valor, SEM_ESCOLHA)
  assert.equal(resumoDaEscolha(s, textos), null, 'nada de "Usa Opus" para um modelo que a conta não tem')
  assert.deepEqual(s.semEscolha, { chave: 'fundo.seletor.salvaModelo', params: { conta: 'Conta a', modelo: 'opus-removido' } })
  assert.equal(modeloNaConta(s.contas[0], 'opus-removido'), false, 'a linha segue desabilitada nessa conta')
  assert.deepEqual(salvo, antes, 'sem troca automática: o rascunho salvo continua o mesmo')
  // Opção que deixou de existir: o modelo existe, a opção não.
  const semOpcao = monta([comOpcao], rascunho({ mode: 'custom', accountId: 'a', modelId: 'sonnet', modelOptions: { effort: 'high' } }))
  assert.deepEqual(semOpcao.valor, SEM_ESCOLHA)
  assert.deepEqual(semOpcao.semEscolha, { chave: 'fundo.seletor.salvaOpcao', params: { conta: 'Conta a' } })
  // O mesmo modelo com opção que existe continua escolhido.
  const ok = monta([comOpcao], rascunho({ mode: 'custom', accountId: 'a', modelId: 'sonnet', modelOptions: { effort: 'low' } }))
  assert.equal(ok.valor.model, 'sonnet')
  assert.equal(ok.semEscolha, null)
})

test('P2-2: Automático só existe, é recomendado e vale com receita que o Hub resolve (Haiku no Claude; Codex nunca por enquanto)', () => {
  const soSonnet = conta('a', { models: [{ id: 'sonnet', options: [] }] })
  // Memória: Claude conectado só com Sonnet. O Hub exige `haiku` (config.rs::recipe), então o Automático não vale.
  const m = monta([soSonnet], rascunho({ accountId: 'a' }))
  assert.equal(m.funcao.automatico, null, 'sem linha Automático nem "Recomendado / hoje: Haiku"')
  assert.deepEqual(m.valor, SEM_ESCOLHA, 'a escolha automática salva não vem marcada')
  assert.equal(resumoDaEscolha(m, textos), null)
  assert.deepEqual(m.semEscolha, { chave: 'fundo.seletor.automaticoFora', params: { conta: 'Conta a' } })
  assert.equal(fraseDaFuncao('memory_consolidation', resumoDaEscolha(m, textos), true, 'leve', 30), null, 'sem frase "Usa …"')
  // Visão: Codex conectado e Automático salvo. O padrão do Codex ainda não é anunciado (`DefaultModelUnknown`).
  const codex = conta('x', { backend: 'codex', models: [{ id: 'gpt', options: [] }] })
  const v = monta([codex], rascunho({ accountId: 'x' }), { functionId: 'vision' })
  assert.equal(v.funcao.automatico, null)
  assert.deepEqual(v.valor, SEM_ESCOLHA)
  assert.equal(v.semEscolha?.chave, 'fundo.seletor.automaticoFora')
  // Caminho do clique: duas contas Claude, só uma com Haiku. A linha existe, mas na conta sem Haiku o clique só explica.
  const duas = monta([conta('boa'), soSonnet], rascunho({ accountId: 'boa' }))
  assert.equal(duas.funcao.automatico?.backend, 'claude')
  const claude = duas.provedores.find((p) => p.id === 'claude')!
  const clique = (marcada: string) => intencaoDeTroca({ atual: { backend: '', accountId: null }, aba: 'claude', provedor: claude, model: null, contas: duas.contas, marcada })
  assert.deepEqual(clique('a'), { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.automaticoFora', conta: 'Conta a' } })
  assert.equal(clique('boa').tipo, 'emitir', 'na conta com Haiku o clique emite o Automático')
  // O modelo explícito continua escolhível na conta sem Haiku.
  assert.equal(intencaoDeTroca({ atual: { backend: '', accountId: null }, aba: 'claude', provedor: claude, model: 'sonnet', contas: duas.contas, marcada: 'a' }).tipo, 'emitir')
  assert.equal(duas.funcao.automaticoFora('Conta a'), 'Automático não funciona em Conta a')
  // Modo conversa (contas sem o campo `automatico`): "Padrão do motor" continua cabendo em qualquer conta.
  assert.equal(modeloNaConta({ models: [{ id: 'sonnet' }] }, null), true)
})

test('P3-1: motor com conta mas fora do catálogo do chat (login caído) continua no trilho, com as contas e as ações', () => {
  const caiu = conta('x', { backend: 'codex', state: 'disconnected', models: [{ id: 'gpt', options: [] }] })
  const s = monta([conta('a'), caiu], rascunho({ accountId: 'a' }), { functionId: 'vision' })
  const semCodex: Provedor[] = catalogo.filter((p) => p.id === 'claude')
  const s2 = seletorDaFuncao({ data: dados([conta('a'), caiu]), functionId: 'vision', draft: rascunho({ functionId: 'vision', accountId: 'a' }), catalogo: semCodex, desconectada: false, textos, abrirContas: abrir })
  assert.deepEqual(s2.provedores.map((p) => p.id), ['claude', 'codex'])
  assert.deepEqual(s2.provedores.find((p) => p.id === 'codex')!.models, [{ id: 'gpt', name: 'gpt' }])
  assert.deepEqual(s2.funcao.inativas.map((c) => [c.id, c.estado, c.acao?.rotulo]), [['x', 'desconectada', 'Conectar']], 'estado e Conectar à vista')
  assert.deepEqual(s2.contas.map((c) => c.id), ['a'], 'continua sem ser marcável')
  assert.deepEqual(s2.valor, s.valor, 'a elegibilidade e a escolha não mudam')
  // Motor sem conta no Hub não aparece.
  assert.deepEqual(seletorDaFuncao({ data: dados([conta('a')]), functionId: 'vision', draft: rascunho({ accountId: 'a' }), catalogo: [], desconectada: false, textos, abrirContas: abrir }).provedores.map((p) => p.id), ['claude'])
})

test('nomes únicos: título repetido no mesmo motor ganha o fim do id; motores diferentes não colidem', () => {
  const m = rotulosUnicos([conta('ac_1111', { label: 'Empresa' }), conta('ac_2222', { label: 'Empresa' }), conta('ac_3333', { label: 'Empresa', backend: 'codex' }), conta('ac_4444', { label: 'Solo' })], 'Perfil antigo')
  assert.equal(m.get('ac_1111'), 'Empresa #1111')
  assert.equal(m.get('ac_2222'), 'Empresa #2222')
  assert.equal(m.get('ac_3333'), 'Empresa')
  assert.equal(m.get('ac_4444'), 'Solo')
  assert.equal(new Set(m.values()).size, 4)
})

test('frase do que acontece: valores do código; só a memória diz quando roda; sem conta que serve, sem frase', () => {
  const r = { motor: 'Claude', conta: 'joana', modelo: 'Sonnet' }
  assert.equal(MEMORIA_OCIOSA_MIN, 30)
  assert.deepEqual(fraseDaFuncao('memory_consolidation', r, true, 'modelo leve', MEMORIA_OCIOSA_MIN), { chave: 'fundo.frase.memoria', params: { motor: 'Claude', conta: 'joana', modelo: 'modelo leve', minutos: 30 } })
  assert.equal(fraseDaFuncao('memory_consolidation', r, false, 'modelo leve', 30)?.chave === 'fundo.frase.memoria' && (fraseDaFuncao('memory_consolidation', r, false, 'modelo leve', 30) as { params: { modelo: string } }).params.modelo, 'Sonnet', 'a frase acompanha a escolha')
  assert.deepEqual(fraseDaFuncao('bot_naming', r, true, 'modelo leve', 30), { chave: 'fundo.frase.generica', params: { motor: 'Claude', conta: 'joana', modelo: 'modelo leve' } })
  assert.equal(fraseDaFuncao('memory_consolidation', null, true, 'x', 30), null)
})
