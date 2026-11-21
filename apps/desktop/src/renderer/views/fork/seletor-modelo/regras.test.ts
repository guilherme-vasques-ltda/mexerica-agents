import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  abaInicialDaFuncao, marcadaDaFuncao, ehPadraoDoMotor, separarPadrao, alternarFavorito, atalho, corpoTroca, destinoDaTroca, engineDaSessao, erroDaTroca, escolhaCruzada,
  favoritosNoCatalogo, filtrar, intencaoDeTroca, aplicarIntencao, LeituraContas, LIMIAR_CONFIRMAR_TOKENS, modeloNaConta, precisaConfirmar, rotuloChip, tamanhoContexto, type ContaMotor, type Provedor,
} from './regras.ts'

const esforco = { id: 'effort', name: 'Raciocínio', default: 'high', values: [{ id: 'medium', name: 'Médio' }, { id: 'high', name: 'Alto' }] }
const provedores: Provedor[] = [
  { id: 'claude', name: 'Claude', models: [{ id: 'opus', name: 'Claude Opus 5.5', options: [esforco] }, { id: 'haiku', name: 'Claude Haiku 5.5' }] },
  { id: 'codex', name: 'Codex', models: [{ id: 'gpt-6.1-sol', name: 'Codex GPT-6.1 Sol' }] },
]

test('a busca acha por nome ou id, sem acento nem maiúscula', () => {
  const modelos = provedores[0]!.models!
  assert.deepEqual(filtrar(modelos, 'HAI').map((m) => m.id), ['haiku'])
  assert.deepEqual(filtrar(modelos, 'opus').map((m) => m.id), ['opus'])
  assert.deepEqual(filtrar([{ id: 'x', name: 'Rápido' }], 'rapido').map((m) => m.id), ['x'])
  assert.equal(filtrar(modelos, '  ').length, 2)
})

test('favoritos: a estrela liga e desliga; os três primeiros são ⌘1–⌘3; o que saiu do catálogo some', () => {
  let favs = alternarFavorito([], 'codex', 'gpt-6.1-sol')
  favs = alternarFavorito(favs, 'claude', 'opus')
  favs = alternarFavorito(favs, 'claude', 'sumiu')
  favs = alternarFavorito(favs, 'claude', 'haiku')
  assert.deepEqual(favs, ['codex/gpt-6.1-sol', 'claude/opus', 'claude/sumiu', 'claude/haiku'])
  assert.equal(atalho(favs, provedores, 1)?.modelo.id, 'gpt-6.1-sol')
  assert.equal(atalho(favs, provedores, 2)?.modelo.id, 'opus')
  assert.equal(atalho(favs, provedores, 3), null, 'o terceiro favorito saiu do catálogo')
  assert.equal(atalho(favs, provedores, 4), null)
  assert.deepEqual(favoritosNoCatalogo(favs, provedores).map((f) => [f.modelo.id, f.atalho]), [['gpt-6.1-sol', 1], ['opus', 2], ['haiku', null]])
  assert.deepEqual(alternarFavorito(favs, 'claude', 'opus'), ['codex/gpt-6.1-sol', 'claude/sumiu', 'claude/haiku'])
})

test('confirmação D375: só no mesmo provedor, com 50 mil tokens ou mais a reler; sem medida troca direto', () => {
  const opus = { backend: 'claude', model: 'opus' }
  assert.equal(LIMIAR_CONFIRMAR_TOKENS, 50_000)
  assert.equal(precisaConfirmar(opus, { backend: 'claude', model: 'haiku' }, 50_000), true)
  assert.equal(precisaConfirmar(opus, { backend: 'claude', model: 'haiku' }, 49_999), false)
  assert.equal(precisaConfirmar(opus, { backend: 'claude', model: 'haiku' }, null), false, 'medidor oculto: não inventa')
  assert.equal(precisaConfirmar(opus, { backend: 'codex', model: 'gpt-6.1-sol' }, 900_000), false, 'outro provedor leva a passagem')
  assert.equal(precisaConfirmar(opus, opus, 900_000), false, 'só o esforço mudou')
})

test('o chip mostra o modelo e o esforço do catálogo; o contexto só com número real', () => {
  assert.deepEqual(rotuloChip({ backend: 'claude', model: 'opus', modelOptions: {}, fastMode: false }, provedores), { modelo: 'Claude Opus 5.5', motor: 'Claude', esforco: 'Alto' })
  assert.deepEqual(rotuloChip({ backend: 'claude', model: 'opus', modelOptions: { effort: 'medium' }, fastMode: false }, provedores), { modelo: 'Claude Opus 5.5', motor: 'Claude', esforco: 'Médio' })
  assert.deepEqual(rotuloChip({ backend: 'codex', model: null, modelOptions: {}, fastMode: false }, provedores), { modelo: null, motor: 'Codex', esforco: null })
  assert.equal(tamanhoContexto('pt-BR', 1_000_000), '1\u00a0mi')
  assert.equal(tamanhoContexto('pt-BR', 400_000), '400\u00a0mil')
  assert.equal(tamanhoContexto('en', 400_000), '400K')
  assert.equal(tamanhoContexto('pt-BR', null), null)
})

test('sem `engine` legível na sessão, o chip fica só leitura; erros do Hub viram texto leigo', () => {
  assert.equal(engineDaSessao({ id: 's' }), null)
  assert.equal(engineDaSessao({ engine: { backend: 'claude' } }), null, 'sem revisão não há troca com CAS')
  assert.deepEqual(engineDaSessao({ engine: { backend: 'claude', model: 'opus', revision: 3, fastMode: true } }),
    { accountId: null, backend: 'claude', model: 'opus', modelOptions: {}, fastMode: true, revision: 3, source: 'botDefault' })
  assert.equal(erroDaTroca({ message: 'session_revision_conflict: x' }), 'conversa.seletor.erro.conflito')
  assert.equal(erroDaTroca({ message: 'texto interno do motor' }), 'conversa.seletor.erro.falhou')
})

// D372 (R8): outro motor só com a conta destino do cadastro; o pedido produzido é o que o Hub valida.
const conta = (id: string, backend: string, extra: Partial<ContaMotor> = {}): ContaMotor =>
  ({ id, label: `Conta ${id}`, backend, state: 'connected', origin: 'managed', models: [{ id: 'gpt-6.1-sol' }], ...extra })

test('R8: uma conta elegível vem marcada e vai no pedido; corpo com as cinco chaves, revisão e principal', () => {
  const cadastro = [conta('ac_c', 'codex'), conta('ac_x', 'codex', { state: 'disconnected' }), conta('ac_l', 'codex', { origin: 'legacy' }), conta('ac_a', 'claude')]
  const destino = destinoDaTroca('claude', 'codex', cadastro, null)
  assert.equal(destino.tipo, 'conta')
  assert.ok(destino.tipo === 'conta' && destino.contas.length === 1 && destino.conta.id === 'ac_c')
  const escolha = destino.tipo === 'conta' ? escolhaCruzada(destino.conta, 'codex', 'gpt-6.1-sol') : null
  assert.deepEqual(corpoTroca('bot', null, 7, escolha!), {
    botId: 'bot', sessionId: null, expectedRevision: 7,
    engine: { accountId: 'ac_c', backend: 'codex', model: 'gpt-6.1-sol', modelOptions: {}, fastMode: false },
  })
  assert.deepEqual(Object.keys(corpoTroca('bot', 'extra-1', 2, escolha!).engine).sort(), ['accountId', 'backend', 'fastMode', 'model', 'modelOptions'])
  assert.equal(corpoTroca('bot', 'extra-1', 2, escolha!).sessionId, 'extra-1')
  // O "Padrão do motor" vale com a conta; um modelo fora do catálogo comprovado da conta não gera pedido.
  assert.equal(escolhaCruzada(conta('ac_c', 'codex'), 'codex', null)?.accountId, 'ac_c')
  assert.equal(escolhaCruzada(conta('ac_c', 'codex'), 'codex', 'fora-da-conta'), null)
  // Decisão do dono: lista de modelos ausente ou vazia não filtra nem desativa nada (o Hub segue decidindo).
  assert.equal(escolhaCruzada(conta('ac_v', 'codex', { models: [] }), 'codex', 'qualquer')?.accountId, 'ac_v')
  assert.equal(modeloNaConta(undefined, 'qualquer'), true)
  assert.equal(modeloNaConta(conta('ac_c', 'codex'), 'fora-da-conta'), false, 'na conta atual também filtra')
})

test('R8: várias contas exigem a marcação; nenhuma deixa o motor indisponível; nunca a primeira sozinha', () => {
  const duas = [conta('ac_1', 'codex'), conta('ac_2', 'codex')]
  const sem = destinoDaTroca('claude', 'codex', duas, null)
  assert.equal(sem.tipo, 'escolher')
  const marcada = destinoDaTroca('claude', 'codex', duas, 'ac_2')
  assert.ok(marcada.tipo === 'conta' && marcada.conta.id === 'ac_2')
  assert.equal(destinoDaTroca('claude', 'codex', duas, 'ac_alheia').tipo, 'escolher')
  assert.equal(destinoDaTroca('claude', 'codex', [conta('ac_a', 'claude')], null).tipo, 'indisponivel')
  assert.equal(destinoDaTroca('claude', 'codex', [], null).tipo, 'indisponivel', 'cadastro ilegível também')
  assert.equal(destinoDaTroca('claude', 'codex', null, null).tipo, 'carregando')
  assert.equal(destinoDaTroca('claude', 'claude', [], null).tipo, 'mesmo', 'no mesmo motor a conta atual continua')
})

test('o "Default (recommended)" do motor sai da lista e vale como "Padrão do motor"', () => {
  const lista = separarPadrao([{ id: 'default', name: 'Default (recommended)' }, { id: 'opus', name: 'Opus' }])
  assert.deepEqual(lista, { models: [{ id: 'opus', name: 'Opus' }], padrao: 'default' })
  assert.deepEqual(separarPadrao([{ id: 'opus', name: 'Opus' }]), { models: [{ id: 'opus', name: 'Opus' }] })
  assert.equal(ehPadraoDoMotor({ model: null }, undefined), true)
  assert.equal(ehPadraoDoMotor({ model: 'default' }, { padrao: 'default' }), true)
  assert.equal(ehPadraoDoMotor({ model: 'opus' }, { padrao: 'default' }), false)
  const comPadrao = [{ ...provedores[0]!, padrao: 'default' }]
  assert.equal(rotuloChip({ backend: 'claude', model: 'default', modelOptions: {}, fastMode: false }, comPadrao).modelo, null)
})

// N1 (Sol 2973): o pedido só sai com a aba do motor e a conta visíveis ANTES do clique. Favorito e atalho ⌘1–⌘3 passam
// pela mesma intenção que o seletor usa para decidir se emite.
test('N1: favorito/atalho de outro motor primeiro navega; só a ação seguinte, com conta visível, emite', () => {
  const codex = { id: 'codex', name: 'Codex' }
  const atual = { backend: 'claude', accountId: 'ac_a' }
  const uma = [conta('ac_c', 'codex'), conta('ac_a', 'claude', { models: [{ id: 'opus' }] })]
  const base = { atual, provedor: codex, model: 'gpt-6.1-sol', contas: uma, marcada: null }
  // Na aba Favoritos (ou outra aba): nenhuma emissão, mesmo com uma única conta elegível.
  assert.deepEqual(intencaoDeTroca({ ...base, aba: 'favoritos' }), { tipo: 'navegar', motivo: null })
  assert.deepEqual(intencaoDeTroca({ ...base, aba: 'claude' }), { tipo: 'navegar', motivo: null })
  // Com a aba do Codex exibida (a conta já visível e marcada): emite com a conta explícita.
  const visivel = intencaoDeTroca({ ...base, aba: 'codex' })
  assert.ok(visivel.tipo === 'emitir' && visivel.escolha.accountId === 'ac_c' && visivel.escolha.backend === 'codex')
  // Duas contas: navega, e na aba exibida aguarda a escolha; com a marcada, emite essa.
  const duas = [conta('ac_1', 'codex'), conta('ac_2', 'codex')]
  assert.equal(intencaoDeTroca({ ...base, contas: duas, aba: 'favoritos' }).tipo, 'navegar')
  assert.deepEqual(intencaoDeTroca({ ...base, contas: duas, aba: 'codex' }), { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.escolha', motor: 'Codex' } })
  const marcada = intencaoDeTroca({ ...base, contas: duas, aba: 'codex', marcada: 'ac_2' })
  assert.ok(marcada.tipo === 'emitir' && marcada.escolha.accountId === 'ac_2')
  // Zero contas: o favorito navega mostrando o motivo; na aba, nunca emite.
  const nenhuma = { chave: 'conversa.seletor.conta.nenhuma', motor: 'Codex' }
  assert.deepEqual(intencaoDeTroca({ ...base, contas: [], aba: 'favoritos' }), { tipo: 'navegar', motivo: nenhuma })
  assert.deepEqual(intencaoDeTroca({ ...base, contas: [], aba: 'codex' }), { tipo: 'esperar', motivo: nenhuma })
  // Modelo fora da conta: navega explicando; na aba, espera com o motivo.
  const fora = { chave: 'conversa.seletor.conta.modeloFora', conta: 'Conta ac_c' }
  assert.deepEqual(intencaoDeTroca({ ...base, model: 'outro', aba: 'favoritos' }), { tipo: 'navegar', motivo: fora })
  assert.deepEqual(intencaoDeTroca({ ...base, model: 'outro', aba: 'codex' }), { tipo: 'esperar', motivo: fora })
  // Mesmo motor: segue como sempre; fora da conta atual, só o motivo (nenhum pedido).
  const claude = { id: 'claude', name: 'Claude' }
  assert.deepEqual(intencaoDeTroca({ ...base, provedor: claude, model: 'opus', aba: 'favoritos' }), { tipo: 'mesmo' })
  assert.deepEqual(intencaoDeTroca({ ...base, provedor: claude, model: 'haiku', aba: 'favoritos' }), { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.modeloFora', conta: 'Conta ac_a' } })
})

// N1 (aceite da Conversa): o mesmo motor segue DIRETO por favorito/atalho, em qualquer aba (uma emissão, sem etapa de
// Conta; a confirmação D375 de 50 mil tokens continua com o Chip); outro motor primeiro navega, com zero emissões.
test('N1: efeitos aplicados — mesmo motor direto (1 emissão) em qualquer aba; outro motor navega (0) e depois emite (1)', () => {
  const registro = () => {
    const r = { navegar: [] as string[], avisar: 0, emitir: [] as { backend: string; accountId?: string | null }[] }
    return { r, efeitos: { navegar: (m: string) => r.navegar.push(m), avisar: () => { r.avisar++ }, emitir: (e: { backend: string; accountId?: string | null }) => r.emitir.push(e) } }
  }
  const atual = { backend: 'claude', accountId: null }
  const contas = [conta('ac_c', 'codex')]
  const claude = { id: 'claude', name: 'Claude' }
  const codex = { id: 'codex', name: 'Codex' }
  const direta = () => ({ accountId: null, backend: 'claude', model: 'haiku', modelOptions: {}, fastMode: false })
  for (const aba of ['favoritos', 'codex', 'claude']) {
    const { r, efeitos } = registro()
    aplicarIntencao(intencaoDeTroca({ atual, aba, provedor: claude, model: 'haiku', contas, marcada: null }), 'claude', direta, efeitos)
    assert.deepEqual(r, { navegar: [], avisar: 0, emitir: [direta()] }, `mesmo motor direto na aba ${aba}`)
  }
  const fora = registro()
  aplicarIntencao(intencaoDeTroca({ atual, aba: 'favoritos', provedor: codex, model: 'gpt-6.1-sol', contas, marcada: null }), 'codex', direta, fora.efeitos)
  assert.deepEqual(fora.r, { navegar: ['codex'], avisar: 0, emitir: [] }, 'outro motor: navega, nenhuma emissão')
  const visivel = registro()
  aplicarIntencao(intencaoDeTroca({ atual, aba: 'codex', provedor: codex, model: 'gpt-6.1-sol', contas, marcada: null }), 'codex', direta, visivel.efeitos)
  assert.equal(visivel.r.emitir.length, 1)
  assert.deepEqual([visivel.r.emitir[0]!.backend, visivel.r.emitir[0]!.accountId], ['codex', 'ac_c'])
  const sem = registro()
  aplicarIntencao(intencaoDeTroca({ atual, aba: 'codex', provedor: codex, model: 'gpt-6.1-sol', contas: [], marcada: null }), 'codex', direta, sem.efeitos)
  assert.deepEqual(sem.r, { navegar: [], avisar: 1, emitir: [] }, 'zero contas na aba: só o motivo')
})

// Ajuste do dono (10b8): falha ao ler `donnaEngineAccounts` não fica guardada como lista vazia. Tem aviso próprio,
// bloqueia a troca entre motores sem chute, e fechar/reabrir o seletor lê de novo; link/época e resposta tardia valem.
test('contas: falha → aviso próprio e bloqueio; fechar/reabrir relê; link/época e resposta tardia', async () => {
  const pendentes: { client: object; resolver: (l: ContaMotor[]) => void; rejeitar: () => void }[] = []
  let vinculo: { client: object | null; epoch: number } = { client: { id: 'A' }, epoch: 1 }
  let mudou = 0
  const leitura = new LeituraContas<object>(() => vinculo, (client) => new Promise((resolver, rejeitar) => { pendentes.push({ client, resolver, rejeitar: () => rejeitar(new Error('ocupado')) }) }), () => { mudou++ })
  const passo = () => new Promise((r) => setImmediate(r))
  leitura.abrir()
  assert.equal(pendentes.length, 1)
  assert.equal(leitura.estado(), null, 'lendo')
  pendentes[0]!.rejeitar()
  await passo()
  assert.equal(leitura.estado(), 'falhou', 'falha não vira lista vazia')
  assert.equal(destinoDaTroca('claude', 'codex', leitura.estado(), null).tipo, 'falhou')
  assert.deepEqual(intencaoDeTroca({ atual: { backend: 'claude', accountId: null }, aba: 'codex', provedor: { id: 'codex', name: 'Codex' }, model: 'gpt-6.1-sol', contas: leitura.estado(), marcada: null }),
    { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.falhou' } }, 'bloqueia a troca entre motores, sem chute')
  // Reabrir com a falha ainda guardada também tenta de novo; fechar descarta a falha.
  leitura.fechar()
  assert.equal(leitura.estado(), null)
  leitura.abrir()
  assert.equal(pendentes.length, 2, 'reabrir lê de novo')
  pendentes[1]!.resolver([conta('ac_c', 'codex')])
  await passo()
  assert.deepEqual((leitura.estado() as ContaMotor[]).map((c) => c.id), ['ac_c'])
  // MX-6: reabrir relê (Contas de IA pode ter ocultado um modelo); a lista de antes fica à mostra até a nova chegar.
  leitura.fechar()
  leitura.abrir()
  assert.equal(pendentes.length, 3, 'reabrir lê de novo, mesmo com a lista guardada')
  assert.deepEqual((leitura.estado() as ContaMotor[]).map((c) => c.id), ['ac_c'], 'a lista anterior segue à mostra enquanto lê')
  pendentes[2]!.resolver([{ ...conta('ac_c', 'codex'), hiddenModelIds: ['gpt-6.1-sol'] }])
  await passo()
  assert.deepEqual((leitura.estado() as ContaMotor[])[0]!.hiddenModelIds, ['gpt-6.1-sol'], 'o oculto feito em Contas de IA chega ao seletor')
  // A releitura falhou: a lista já lida continua valendo (não vira falha).
  leitura.fechar()
  leitura.abrir()
  pendentes[3]!.rejeitar()
  await passo()
  assert.deepEqual((leitura.estado() as ContaMotor[])[0]!.hiddenModelIds, ['gpt-6.1-sol'])
  // Outro link/época: a lista anterior não vale; uma resposta tardia do pedido antigo é descartada.
  vinculo = { client: vinculo.client, epoch: 2 }
  assert.equal(leitura.estado(), null)
  leitura.abrir()
  assert.equal(pendentes.length, 5)
  vinculo = { client: { id: 'B' }, epoch: 2 }
  pendentes[4]!.resolver([conta('ac_velha', 'codex')])
  await passo()
  assert.equal(leitura.estado(), null, 'resposta do link anterior não entra')
  leitura.abrir()
  pendentes[5]!.resolver([conta('ac_b', 'codex')])
  await passo()
  assert.deepEqual((leitura.estado() as ContaMotor[]).map((c) => c.id), ['ac_b'])
  // Pedido em voo quando o seletor fecha: descartado.
  vinculo = { client: vinculo.client, epoch: 3 }
  leitura.abrir()
  leitura.fechar()
  pendentes[6]!.resolver([conta('ac_fechado', 'codex')])
  await passo()
  assert.equal(leitura.estado(), null)
  assert.ok(mudou >= 3)
})

test('Entrega 3: conta alvo da aba, modelo oculto por conta e troca de conta no mesmo motor', async () => {
  const { contaAlvo, modeloOculto, ocultoNaConta } = await import('./regras.ts')
  const conta = (id: string, backend: string, hiddenModelIds: string[] = []): ContaMotor => ({ id, label: id, backend, state: 'connected', origin: 'managed', models: [], hiddenModelIds })
  const contas = [conta('c1', 'claude', ['haiku']), conta('c2', 'claude'), conta('x1', 'codex', ['gpt-6.1-sol']), conta('x2', 'codex', ['gpt-6.1-sol'])]
  const atual = { backend: 'claude', accountId: 'c1' }
  // A aba do motor da conversa mostra a conta dela; a marcada vence; noutro motor com várias, ninguém.
  assert.equal(contaAlvo(atual, 'claude', contas, null)?.id, 'c1')
  assert.equal(contaAlvo(atual, 'claude', contas, 'c2')?.id, 'c2')
  assert.equal(contaAlvo(atual, 'codex', contas, null), null)
  assert.equal(contaAlvo(atual, 'codex', [contas[2]!], null)?.id, 'x1', 'a única elegível')
  assert.equal(contaAlvo(atual, 'claude', null, null), null)
  // Oculto vale por conta; sem lista no DTO (Hub atual), nada some.
  assert.equal(ocultoNaConta(contas[0], 'haiku'), true)
  assert.equal(ocultoNaConta({}, 'haiku'), false)
  assert.equal(modeloOculto(atual, 'claude', 'haiku', contas, null), true)
  assert.equal(modeloOculto(atual, 'claude', 'haiku', contas, 'c2'), false, 'na outra conta ele aparece')
  assert.equal(modeloOculto(atual, 'codex', 'gpt-6.1-sol', contas, null), true, 'oculto em todas as contas do outro motor')
  assert.equal(modeloOculto(atual, 'codex', 'gpt-6.1-sol', [contas[2]!, conta('x3', 'codex')], null), false, 'alguma conta ainda o mostra')
  assert.equal(modeloOculto(atual, 'claude', 'haiku', 'falhou', null), false)
  // Mesmo motor, outra conta marcada na aba exibida: o pedido sai com a conta explícita; fora da aba, segue a atual.
  const provedor = provedores[0]!
  assert.deepEqual(intencaoDeTroca({ atual, aba: 'claude', provedor, model: 'opus', contas, marcada: 'c2' }), { tipo: 'emitir', escolha: { accountId: 'c2', backend: 'claude', model: 'opus', modelOptions: {}, fastMode: false } })
  assert.deepEqual(intencaoDeTroca({ atual, aba: 'favoritos', provedor, model: 'opus', contas, marcada: 'c2' }), { tipo: 'mesmo' })
  assert.deepEqual(intencaoDeTroca({ atual, aba: 'claude', provedor, model: 'opus', contas, marcada: 'c1' }), { tipo: 'mesmo' })
})

test('Entrega 3: o filtro em memo por motor devolve o mesmo que `filtrar`, sem refazer a lista da mesma busca', async () => {
  const { filtrarMemo, ocultosDoMotor, modeloOculto } = await import('./regras.ts')
  const modelos = Array.from({ length: 150 }, (_, i) => ({ id: `m-${i}`, name: i % 7 ? `Sabiá ${i}` : `Rápido ${i}` }))
  assert.equal(filtrarMemo(modelos, '  '), modelos, 'sem texto, a própria lista')
  for (const q of ['rapido', 'SABIÁ 1', 'm-14', 'nada', 'r']) assert.deepEqual(filtrarMemo(modelos, q), filtrar(modelos, q), q)
  assert.equal(filtrarMemo(modelos, 'rapido'), filtrarMemo(modelos, ' RÁPIDO '), 'a mesma busca devolve o mesmo vetor')
  // Outra lista (outro motor) tem o memo dela.
  const outros = [{ id: 'x', name: 'Rápido' }]
  assert.deepEqual(filtrarMemo(outros, 'rapido'), outros)
  assert.equal(filtrarMemo(modelos, 'rapido').length, 22)
  // O oculto resolvido uma vez por motor dá o mesmo que modelo a modelo.
  const conta = (id: string, backend: string, hiddenModelIds?: string[]): ContaMotor => ({ id, label: id, backend, state: 'connected', origin: 'managed', models: [], ...(hiddenModelIds ? { hiddenModelIds } : {}) })
  const contas = [conta('c1', 'claude', ['a', 'b']), conta('c2', 'claude'), conta('x1', 'codex', ['a']), conta('x2', 'codex', ['a', 'b'])]
  const atual = { backend: 'claude', accountId: 'c1' }
  for (const [backend, marcada] of [['claude', null], ['claude', 'c2'], ['codex', null], ['codex', 'x2'], ['opencode', null]] as const) {
    const some = ocultosDoMotor(atual, backend, contas, marcada)
    for (const m of ['a', 'b', 'c']) assert.equal(some(m), modeloOculto(atual, backend, m, contas, marcada), `${backend}/${marcada}/${m}`)
  }
  assert.deepEqual(['a', 'b', 'c'].map(ocultosDoMotor(atual, 'codex', contas, null)), [true, false, false], 'sem conta marcada, só o oculto em todas')
  assert.equal(ocultosDoMotor(atual, 'claude', null, null)('a'), false)
})

test('função de fundo: sem "mesmo motor"; a conta salva vem marcada e a que a pessoa tocou vence', () => {
  const salva = { backend: 'claude', accountId: 'c1' }
  assert.equal(marcadaDaFuncao(salva, 'claude', {}), 'c1', 'no motor da escolha, a conta salva')
  assert.equal(marcadaDaFuncao(salva, 'claude', { claude: 'c2' }), 'c2', 'a tocada vence')
  assert.equal(marcadaDaFuncao(salva, 'codex', {}), null, 'outro motor: nenhuma marcada, a pessoa escolhe')
  assert.equal(marcadaDaFuncao({ backend: '', accountId: null }, 'claude', {}), null, 'sem escolha: nada marcado')
  // Com o motor atual vazio ('' = sem escolha), toda intenção leva a conta explícita: nunca `mesmo`.
  const sem = { backend: '', accountId: null }
  const claude: Provedor = { id: 'claude', name: 'Claude', models: [{ id: 'haiku', name: 'Haiku' }] }
  const haiku = { models: [{ id: 'haiku' }] }
  const duas = [conta('c1', 'claude', haiku), conta('c2', 'claude', haiku)]
  assert.deepEqual(intencaoDeTroca({ atual: sem, aba: 'claude', provedor: claude, model: 'haiku', contas: [duas[0]!], marcada: null }),
    { tipo: 'emitir', escolha: { accountId: 'c1', backend: 'claude', model: 'haiku', modelOptions: {}, fastMode: false } }, 'única conta pronta')
  assert.deepEqual(intencaoDeTroca({ atual: sem, aba: 'claude', provedor: claude, model: null, contas: duas, marcada: 'c2' }),
    { tipo: 'emitir', escolha: { accountId: 'c2', backend: 'claude', model: null, modelOptions: {}, fastMode: false } }, 'Automático (modelo vazio) com a conta marcada')
  assert.deepEqual(intencaoDeTroca({ atual: sem, aba: 'claude', provedor: claude, model: 'haiku', contas: duas, marcada: null }),
    { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.escolha', motor: 'Claude' } }, 'várias e nenhuma marcada: nada é enviado')
  assert.deepEqual(intencaoDeTroca({ atual: sem, aba: 'claude', provedor: claude, model: 'haiku', contas: [], marcada: null }),
    { tipo: 'esperar', motivo: { chave: 'conversa.seletor.conta.nenhuma', motor: 'Claude' } }, 'nenhuma conta pronta: o motor fica indisponível')
  assert.equal(intencaoDeTroca({ atual: sem, aba: 'favoritos', provedor: claude, model: 'haiku', contas: duas, marcada: 'c1' }).tipo, 'navegar', 'favorito de outra aba só navega')
})

test('função de fundo: a aba inicial é a do motor da escolha, nunca um motor recusado', () => {
  const ps = [{ id: 'claude' }, { id: 'codex' }]
  const recusados = { codex: { curto: 'Só roda no Claude', motivo: '…' } }
  assert.equal(abaInicialDaFuncao('claude', ps, recusados), 'claude')
  assert.equal(abaInicialDaFuncao('codex', ps, recusados), 'claude', 'a escolha salva num motor recusado abre no primeiro aceito')
  assert.equal(abaInicialDaFuncao('', ps, recusados), 'claude', 'sem escolha')
  assert.equal(abaInicialDaFuncao('codex', ps, {}), 'codex', 'aceito')
  assert.equal(abaInicialDaFuncao('', [], {}), '', 'sem motores')
})

test('favorito de motor que o Hub deixou de oferecer não aparece nem pode ser escolhido', () => {
  const semKilo: Provedor[] = [
    { id: 'claude', name: 'Claude', models: [{ id: 'modelo-a', name: 'Modelo A' }] },
    { id: 'codex', name: 'Codex', models: [{ id: 'modelo-b', name: 'Modelo B' }] },
  ]
  const favoritos = ['kilo/modelo', 'codex/modelo-b']
  assert.deepEqual(favoritosNoCatalogo(favoritos, semKilo).map((f) => f.modelo.id), ['modelo-b'])
  assert.equal(atalho(favoritos, semKilo, 1), null, 'o atalho do favorito sumido não escolhe nada')
  assert.equal(atalho(favoritos, semKilo, 2)?.modelo.id, 'modelo-b')
})
