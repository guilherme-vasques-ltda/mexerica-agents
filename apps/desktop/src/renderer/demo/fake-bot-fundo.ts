// Entrega 4 (MX-7), só na demo: `?botfundo=<cena>` liga o Bot de fundo e a Atividade com dados fictícios, no formato
// do Hub (`host/src/fork/modelos_funcao/api.rs`, `host/src/fork/background_history.rs`). Cenas: normal (12 registros),
// poucos (4), muitos (50, o máximo que o Hub guarda), vazio (nenhum), falha (a leitura das funções falha até
// `donnabotEditorDemo.host.botFundo.falhar = false`) e antigo (Hub sem `consumer`). Sem conexão: `donnabotEditorDemo.setOffline(true)`.
// Nada é executado; salvar só muda esta memória.
type Fail = (message: string) => unknown

const pedida = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('botfundo')
export const botFundoDemo = () => pedida !== null

const CONTA = 'demo-claude'
const CONTAS = [{ id: CONTA, label: 'Assinatura pessoal', backend: 'claude', state: 'connected', billing: 'subscription',
  models: [{ id: 'claude-haiku-5-5', options: [{ id: 'reasoning_effort', values: ['low', 'medium', 'high'], default: null }] }, { id: 'claude-sonnet-5-5', options: [] }] },
{ id: 'demo-codex', label: 'Codex trabalho', backend: 'codex', state: 'connected', billing: 'subscription', models: [{ id: 'gpt-6.1-luna', options: [] }] }]
// textos-ok: dado simulado do Hub (o nome vem do catálogo do Hub em português), não é rótulo da tela
const FUNCOES: [string, string][] = [['memory_consolidation', 'Organizar memória'], ['bot_naming', 'Títulos'], ['context_compression', 'Compactar contexto'], ['vision', 'Visão'], ['message_triage', 'Mensagens durante o trabalho'], ['skill_selection', 'Selecionar skills'], ['skills_hub', 'Hub de skills'], ['approval_judge', 'Aprovações'], ['mcp_routing', 'Roteamento de ferramentas MCP'], ['bot_draft', 'Rascunho de novo bot'], ['voice_live', 'Conversa por voz'], ['speech_to_text', 'Ditado'], ['text_to_speech', 'Ler respostas']]
const VOZ = ['voice_live', 'speech_to_text', 'text_to_speech']
const CODEX = ['vision', 'context_compression', 'bot_draft']

const BOTS = ['sprint', 'byte', 'crivo', 'prosa']
const FUNCOES_HIST = ['memory', 'titles', 'memory', 'daily_summary', 'titles', 'memory'] as const
const STATUS = ['applied', 'unchanged', 'applied', 'proposed', 'applied', 'response_received', 'deferred', 'applied', 'failed', 'unchanged'] as const
const ETAPAS = ['account', 'execution', null, 'usage'] as const

/** `n` registros a cada ~47 min, com uso e falhas variados, na ordem do Hub: do mais antigo ao mais novo (ele acrescenta no fim). */
export function historicoDemo(n: number, agora = Date.now()) {
  return Array.from({ length: n }, (_, i) => {
    const status = STATUS[i % STATUS.length]!
    const fn = FUNCOES_HIST[i % FUNCOES_HIST.length]!
    const semUso = status === 'deferred' || (status === 'failed' && i % 3 === 0)
    return {
      id: `demo-atividade-${i}`, function: fn, botId: BOTS[(i * 3) % BOTS.length]!, status, at: agora - 4 * 60_000 - i * 47 * 60_000,
      effectiveModels: status === 'deferred' ? [] : fn === 'titles' ? ['claude-haiku-5-5'] : ['claude-haiku-5-5'],
      ...(semUso ? {} : { usage: { inputTokens: 1800 + ((i * 7919) % 9000), outputTokens: 120 + ((i * 131) % 900), cachedInputTokens: (i % 4) * 1200, cachedWriteTokens: (i % 5) * 300 } }),
      failureStage: status === 'failed' ? ETAPAS[i % ETAPAS.length] : null,
    }
  }).reverse()
}

export class FakeBotFundo {
  revision = 4
  falhar = pedida === 'falha'
  private selections: Record<string, unknown> = Object.fromEntries(FUNCOES.map(([id]) => [id,
    VOZ.includes(id) ? { mode: 'managedInVoice' }
      : id === 'memory_consolidation' ? { mode: 'custom', accountId: CONTA, modelId: 'claude-haiku-5-5', modelOptions: { reasoning_effort: 'low' } }
        : CODEX.includes(id) ? { mode: 'automatic', accountId: 'demo-codex', binding: { accountId: 'demo-codex', modelId: 'gpt-6.1-luna', modelOptions: {} } }
          : { mode: 'automatic', accountId: CONTA, binding: { accountId: CONTA, modelId: 'claude-haiku-5-5', modelOptions: { reasoning_effort: 'low' } } }]))
  private history = historicoDemo(pedida === 'muitos' ? 50 : pedida === 'poucos' ? 4 : pedida === 'vazio' ? 0 : 12)

  atende(method: string) {
    return botFundoDemo() && ['functionModels', 'saveFunctionModel', 'checkFunctionModel', 'backgroundSpaceSelections', 'saveBackgroundSpaceSelection', 'backgroundHistory'].includes(method)
  }

  call(method: string, b: Record<string, unknown>, fail: Fail): unknown {
    switch (method) {
      case 'functionModels': if (this.falhar) throw fail('timeout'); return this.functions()
      case 'checkFunctionModel': return this.resolution(String(b.functionId))
      case 'backgroundHistory': return structuredClone(this.history)
      case 'backgroundSpaceSelections': return { execution: 'awaiting_executor', selections: [] }
      case 'saveFunctionModel':
        if (b.expectedRevision !== this.revision) throw fail('revision_changed')
        this.selections[String(b.functionId)] = b.selection
        this.revision++
        return { document: this.document(), execution: 'partial' }
      default: throw fail('background_space_selection_unavailable')
    }
  }

  private document() { return { schemaVersion: 1, revision: this.revision, functions: structuredClone(this.selections) } }

  private resolution(id: string) {
    const s = this.selections[id] as { mode: string; accountId?: string; modelId?: string; modelOptions?: Record<string, string>; binding?: { accountId: string; modelId: string; modelOptions: Record<string, string> } }
    const binding = s.mode === 'custom' ? { accountId: s.accountId!, modelId: s.modelId!, modelOptions: s.modelOptions ?? {} } : s.binding
    return binding ? { status: 'available', resolved: { effective: binding } } : { status: 'unavailable', unavailable: { reason: 'account_choice_required' } }
  }

  private functions() {
    return { document: this.document(), accounts: CONTAS, accountsIssue: null, execution: 'partial',
      functions: FUNCOES.map(([id, name]) => ({
        definition: { id, name, managedInVoice: VOZ.includes(id), recipe: 'demo' }, selection: this.selections[id],
        resolution: VOZ.includes(id) ? { status: 'unavailable', unavailable: { reason: 'context_denied' } } : this.resolution(id),
        ...(pedida === 'antigo' ? {} : { consumer: id === 'memory_consolidation' || id === 'bot_naming' ? 'background_worker' : 'awaiting_implementation' }),
      })) }
  }
}
