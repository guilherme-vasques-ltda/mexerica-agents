// DON-907 C9: `?seletor=1` simula o Hub da UI01b para o seletor de modelo: `sessionActions` com `engine`, o `engine`
// em cada sessão e `sessionEngineSet`, com um catálogo fictício por motor. Nada disto existe no Hub atual.
export const seletorDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('seletor') === '1'

const esforco = (padrao: string) => ({
  id: 'effort', name: 'Esforço', default: padrao,
  values: [{ id: 'low', name: 'Baixo' }, { id: 'medium', name: 'Médio' }, { id: 'high', name: 'Alto' }, { id: 'xhigh', name: 'Extra alto' }, { id: 'max', name: 'Máx.' }],
})

// Entrega 3 da revisão do app (proposta visual): `?seletor=1&modelos=e3` troca o catálogo por um do tamanho da vida real
// (3 motores, 44 modelos, 2 contas no mesmo motor, modelos ocultos por conta). `&emuso=oculto` põe a conversa num modelo
// oculto; `&falha=<motor>` faz a leitura dos modelos desse motor falhar; `&poucos=<motor>` deixa só 3 modelos nele. Tudo fictício.
const param = (nome: string) => (typeof location !== 'undefined' ? new URLSearchParams(location.search).get(nome) : null)
export const e3Demo = () => seletorDemo() && param('modelos') === 'e3'
export const falhaDemo = (backend: string) => e3Demo() && param('falha') === backend
/** `&hub=antigo`: o Hub de antes dos modelos ocultos: as contas vêm sem `hiddenModelIds` e o método não existe. */
export const hubAntigoDemo = () => e3Demo() && param('hub') === 'antigo'
/** `&grande=<n>`: n modelos fictícios repartidos pelos 3 motores, para medir a lista (docs/reference/design/propostas/modelos-contas-ia/medir.mjs). */
const grande = () => (e3Demo() ? Math.max(0, Number(param('grande')) || 0) : 0)
const MOTORES_E3 = ['claude', 'codex', 'opencode']
const FAMILIAS = ['Sabiá', 'Jatobá', 'Cajá', 'Açaí', 'Ipê', 'Uirapuru', 'Pequi', 'Jequitibá']
const grandes = new Map<string, ReturnType<typeof linha>[]>()
function modelosGrandes(backend: string, n: number) {
  const k = `${backend}/${n}`
  const pos = MOTORES_E3.indexOf(backend)
  const quantos = pos < 0 ? 0 : Math.floor(n / 3) + (pos < n % 3 ? 1 : 0)
  if (!grandes.has(k)) grandes.set(k, Array.from({ length: quantos }, (_, i) => linha(`${backend}-m${i + 1}`, `${FAMILIAS[i % FAMILIAS.length]} ${Math.floor(i / FAMILIAS.length) + 1}.${i % 10} ${backend}`, (1 + (i % 10)) * 100_000, i % 3 ? undefined : 'medium')))
  return grandes.get(k)!
}

const linha = (id: string, name: string, contextTokens: number, padrao?: string) => ({ id, name, contextTokens, ...(padrao ? { options: [esforco(padrao)] } : {}) })
const CATALOGO_E3: Record<string, { currentModelId: string; fastMode: boolean; models: ReturnType<typeof linha>[] }> = {
  claude: { currentModelId: 'claude-opus-5-5', fastMode: false, models: [
    linha('claude-opus-5-5', 'Opus 5.5', 1_000_000, 'high'), linha('claude-sonnet-5-5', 'Sonnet 5.5', 1_000_000, 'high'), linha('claude-haiku-5-5', 'Haiku 5.5', 200_000),
    linha('claude-opus-5-2', 'Opus 5.2', 1_000_000, 'high'), linha('claude-sonnet-5-2', 'Sonnet 5.2', 1_000_000, 'medium'), linha('claude-haiku-5-2', 'Haiku 5.2', 200_000),
    linha('claude-opus-5', 'Opus 5', 500_000, 'high'), linha('claude-sonnet-5', 'Sonnet 5', 500_000, 'medium'), linha('claude-haiku-5', 'Haiku 5', 200_000),
    linha('claude-opus-4-8', 'Opus 4.8', 200_000, 'high'), linha('claude-sonnet-4-8', 'Sonnet 4.8', 200_000, 'medium'),
    linha('claude-opus-4-6', 'Opus 4.6', 200_000, 'high'), linha('claude-sonnet-4-6', 'Sonnet 4.6', 200_000), linha('claude-haiku-4-6', 'Haiku 4.6', 200_000),
  ] },
  codex: { currentModelId: 'gpt-6.1-sol', fastMode: true, models: [
    linha('gpt-6.1-sol', '6.1 Sol', 400_000, 'medium'), linha('gpt-6.1-terra', '6.1 Terra', 400_000, 'medium'), linha('gpt-6.1-lua', '6.1 Lua', 200_000),
    linha('gpt-6-astra', '6 Astra', 400_000, 'medium'), linha('gpt-6-astra-mini', '6 Astra mini', 200_000), linha('gpt-6-codex', '6 Codex', 400_000, 'high'),
    linha('gpt-6-codex-mini', '6 Codex mini', 200_000, 'medium'), linha('gpt-5.6-sol', '5.6 Sol', 400_000, 'medium'), linha('gpt-5.6-terra', '5.6 Terra', 400_000, 'medium'),
    linha('gpt-5.5', '5.5', 272_000, 'medium'), linha('gpt-5.5-mini', '5.5 mini', 128_000), linha('gpt-5.4', '5.4', 272_000, 'medium'),
    linha('gpt-5.3-codex', '5.3 Codex', 272_000, 'high'), linha('gpt-5.2', '5.2', 128_000),
  ] },
  opencode: { currentModelId: 'kimi-k3', fastMode: false, models: [
    linha('kimi-k3', 'Kimi K3', 256_000), linha('qwen-4-coder', 'Qwen 4 Coder', 1_000_000), linha('qwen-4-coder-flash', 'Qwen 4 Coder Flash', 256_000), linha('glm-5.1', 'GLM 5.1', 200_000),
    linha('glm-5.1-air', 'GLM 5.1 Air', 128_000), linha('deepseek-v4', 'DeepSeek V4', 128_000), linha('deepseek-v4-r', 'DeepSeek V4 Raciocínio', 128_000, 'medium'), linha('minimax-m3', 'MiniMax M3', 200_000),
    linha('devstral-3', 'Devstral 3', 256_000), linha('devstral-3-small', 'Devstral 3 Small', 128_000), linha('llama-5-70b', 'Llama 5 70B', 128_000), linha('grok-build-2', 'Grok Build 2', 256_000),
    linha('gemini-3.5-pro', 'Gemini 3.5 Pro', 1_000_000, 'high'), linha('gemini-3.5-flash', 'Gemini 3.5 Flash', 1_000_000), linha('nova-coder-2', 'Nova Coder 2', 300_000), linha('big-pickle', 'Big Pickle', 200_000),
  ] },
}
const ids = (backend: string, n?: number) => CATALOGO_E3[backend]!.models.slice(0, n).map((m) => m.id)

/** Catálogo fictício (exemplos, não dados reais da frente Modelos e contas). */
export function modelosDemo(backend: string) {
  if (e3Demo()) {
    // textos-ok: erro sintético da demo, nunca mostrado (o seletor mostra o próprio texto).
    if (falhaDemo(backend)) throw new Error('demo_timeout: leitura dos modelos indisponível')
    const base = CATALOGO_E3[backend] ?? { currentModelId: '', fastMode: false, models: [] }
    const cat = grande() ? { ...base, models: [...base.models.slice(0, 1), ...modelosGrandes(backend, grande() - 3)] } : base
    // `&poucos=<motor>`: só 3 modelos nesse motor, para ver o painel de altura fixa com sobra.
    return param('poucos') === backend ? { ...cat, models: cat.models.slice(0, 3) } : cat
  }
  return backend === 'codex'
    ? { currentModelId: 'gpt-6.1-sol', fastMode: true, models: [
      { id: 'gpt-6.1-sol', name: 'Codex GPT-6.1 Sol', contextTokens: 400_000, options: [esforco('medium')] },
      { id: 'gpt-6.1-terra', name: 'Codex GPT-6.1 Terra', contextTokens: 400_000, options: [esforco('medium')] },
    ] }
    : { currentModelId: 'claude-opus-5-5', fastMode: false, models: [
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', contextTokens: 1_000_000, options: [esforco('high')] },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', contextTokens: 1_000_000, options: [esforco('high')] },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', contextTokens: 200_000 },
    ] }
}

type Engine = { accountId: string | null; backend: string; model: string | null; modelOptions: Record<string, string>; fastMode: boolean; source: string; revision: number }
const engines = new Map<string, Engine>()

// MX-6: modelos ocultados por conta no cadastro fictício, para a tela de Contas de IA.
const ocultosDemo = new Map<string, Set<string>>()

/** `donnaEngineAccountModelVisibility` fictício: um gesto = um modelo, idempotente como no Hub. */
export function visibilidadeDemo(engineAccountId: string, modelId: string, hidden: boolean) {
  const set = ocultosDemo.get(engineAccountId) ?? new Set<string>()
  if (hidden) set.add(modelId)
  else set.delete(modelId)
  ocultosDemo.set(engineAccountId, set)
}

/**
 * Contas de IA com ações à vista (`&contas=acoes`, junto de `&modelos=e3` para os bots existirem): a tela do dono com
 * conta nova sem login, perfis antigos (conectado e desconectados), uma conta em uso por bots e uma sem uso. Fictício.
 */
const acoesDemo = () => seletorDemo() && param('contas') === 'acoes'
const removidasDemo = new Set<string>()
const contaAcoes = (id: string, label: string, backend: 'claude' | 'codex', origin: 'managed' | 'legacy', state: 'connected' | 'unverified' | 'unavailable' | 'disconnected', billing: 'subscription' | 'unverified', botIds: string[] = []) =>
  ({ id: `ac_${id.repeat(64)}`, label, brand: backend === 'claude' ? 'Anthropic' : 'OpenAI', backend, billing, state, models: [], revision: 1, origin, botIds, hiddenModelIds: [] as string[] })
const contasAcoes = [
  contaAcoes('1', 'Empresa', 'codex', 'managed', 'unverified', 'unverified'),
  contaAcoes('2', 'Claude pessoal', 'claude', 'managed', 'connected', 'subscription', ['sprint', 'crivo']),
  contaAcoes('3', 'Codex trabalho', 'codex', 'managed', 'connected', 'subscription'),
  contaAcoes('4', 'Perfil antigo', 'claude', 'legacy', 'connected', 'subscription'),
  contaAcoes('5', 'Perfil antigo', 'claude', 'legacy', 'disconnected', 'unverified'),
  contaAcoes('6', 'Perfil antigo', 'codex', 'legacy', 'disconnected', 'unverified'),
  contaAcoes('7', 'Codex pessoal', 'codex', 'managed', 'unavailable', 'subscription'),
]
/** `donnaEngineAccountRemove` fictício, com as mesmas recusas do Hub: em uso, perfil antigo conectado ou conta que não existe. */
export function removerContaDemo(engineAccountId: string) {
  const conta = contasAcoes.find((c) => c.id === engineAccountId && !removidasDemo.has(c.id))
  if (!conta) throw new Error('account_not_found: conta não encontrada.')
  if (conta.origin === 'legacy' && conta.state !== 'disconnected') throw new Error('legacy_preserved: configuração legada deve ser mantida.')
  if (conta.botIds.length) throw new Error('account_in_use: remova os vínculos dos bots primeiro.')
  removidasDemo.add(conta.id)
}

/** O `engine` de uma sessão (`chatId/sessionId`), nascendo com o padrão inicial do bot. */
export function engineDemo(key: string, revision: number): Engine {
  const e = engines.get(key) ?? (e3Demo()
    ? { accountId: CONTA_E3.claudePessoal, backend: 'claude', model: param('emuso') === 'oculto' ? 'claude-opus-4-6' : 'claude-opus-5-5', modelOptions: { effort: 'high' }, fastMode: false, source: 'botDefault', revision }
    : { accountId: null, backend: 'claude', model: 'claude-opus-5-5', modelOptions: { effort: 'high' }, fastMode: false, source: 'botDefault', revision })
  engines.set(key, e)
  return e
}

/**
 * D372 (R8): o cadastro fictício de contas (`donnaEngineAccounts`), só no `?seletor=1`. Codex tem uma conta (aparece
 * visível no seletor) ou duas com `&contas=2` (a pessoa escolhe). Contas e modelos de exemplo, sem dados reais.
 */
export function contasDemo() {
  if (acoesDemo()) return { accounts: contasAcoes.filter((c) => !removidasDemo.has(c.id)) }
  // Catálogo grande: as contas vêm sem lista de modelos (nada é filtrado pela conta).
  if (e3Demo()) {
    // Os ocultos de saída entram uma vez no mesmo cadastro que `visibilidadeDemo` altera. Hub antigo: sem o campo.
    if (!semeado) { semeado = true; for (const c of contasE3) ocultosDemo.set(c.id, new Set(c.hiddenModelIds)) }
    return { accounts: contasE3.map(({ hiddenModelIds: _, ...conta }) => ({ ...conta, ...(grande() ? { models: [] } : {}), ...(hubAntigoDemo() ? {} : { hiddenModelIds: [...(ocultosDemo.get(conta.id) ?? [])] }) })) }
  }
  const duas = typeof location !== 'undefined' && new URLSearchParams(location.search).get('contas') === '2'
  const conta = (id: string, label: string, backend: 'claude' | 'codex', models: string[]) =>
    ({ id, label, brand: backend === 'claude' ? 'Anthropic' : 'OpenAI', backend, billing: 'subscription', state: 'connected', models: models.map((m) => ({ id: m, options: [] })), hiddenModelIds: [...(ocultosDemo.get(id) ?? [])], revision: 1, origin: 'managed', botIds: [] })
  return { accounts: [
    conta(`ac_${'c'.repeat(64)}`, 'Claude pessoal', 'claude', ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5']),
    conta(`ac_${'d'.repeat(64)}`, 'Codex trabalho', 'codex', ['gpt-6.1-sol']),
    ...(duas ? [conta(`ac_${'e'.repeat(64)}`, 'Codex pessoal', 'codex', ['gpt-6.1-sol', 'gpt-6.1-terra'])] : []),
  ] }
}

/** Entrega 3: as contas do cenário grande. Duas no Claude e duas no Codex (uma por chave, ainda sem lista de modelos). */
const CONTA_E3 = { claudePessoal: `ac_${'a'.repeat(64)}`, claudeTrabalho: `ac_${'b'.repeat(64)}`, codexTrabalho: `ac_${'c'.repeat(64)}`, codexApi: `ac_${'d'.repeat(64)}` }
const contaE3 = (id: string, label: string, backend: 'claude' | 'codex', billing: 'subscription' | 'paid_api', models: string[], botIds: string[], hiddenModelIds: string[]) =>
  ({ id, label, brand: backend === 'claude' ? 'Anthropic' : 'OpenAI', backend, billing, state: 'connected', models: models.map((m) => ({ id: m, options: [] })), revision: 1, origin: 'managed', botIds, hiddenModelIds })
let semeado = false
const contasE3 = [
  contaE3(CONTA_E3.claudePessoal, 'Claude pessoal', 'claude', 'subscription', ids('claude'), ['sprint', 'crivo'], ['claude-opus-4-8', 'claude-sonnet-4-8', 'claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-6']),
  contaE3(CONTA_E3.claudeTrabalho, 'Claude trabalho', 'claude', 'subscription', ids('claude', 9), [], []),
  contaE3(CONTA_E3.codexTrabalho, 'Codex trabalho', 'codex', 'subscription', ids('codex'), ['byte', 'prosa'], ['gpt-5.5', 'gpt-5.5-mini', 'gpt-5.4', 'gpt-5.3-codex', 'gpt-5.2']),
  contaE3(CONTA_E3.codexApi, 'Codex por chave', 'codex', 'paid_api', [], [], []),
]
/** Os modelos que os bots da demo usam no cenário grande (ids do catálogo acima). */
export const MODELOS_DOS_BOTS_E3: Record<string, string | null> = { sprint: 'claude-opus-5-5', crivo: 'claude-sonnet-5-5', byte: 'gpt-6.1-sol', prosa: null }

/** `sessionEngineSet`: grava com CAS e devolve a troca. Como o Hub, outro motor exige a conta destino. */
export function trocarDemo(key: string, expected: number, engine: Omit<Engine, 'source' | 'revision'>, revision: number) {
  const atual = engines.get(key)
  if (!atual || atual.revision !== expected) throw new Error('session_revision_conflict: a sessão mudou')
  if (engine.backend !== atual.backend && !engine.accountId) throw new Error('engine_not_allowed: outro motor exige a conta destino')
  const novo = { ...engine, source: 'own', revision }
  engines.set(key, novo)
  return { from: { backend: atual.backend, model: atual.model }, to: { backend: novo.backend, model: novo.model }, kind: atual.backend === novo.backend ? 'model' : 'provider', appliesAt: 'now' }
}
