import type { Binding, FunctionSnapshot, Selection, SpaceSnapshot } from './model'

/** Fixture opt-in: importada manualmente no Vite. Histórico sintético só por setHistory; não executa modelos. */
export function installBackgroundFixture(host: { call(method: string, body: Record<string, unknown>): Promise<unknown>; emit(event: object): void }) {
  const original = host.call.bind(host)
  const requests: { method: string; body: Record<string, unknown> }[] = []
  const accountId = `ac_${'a'.repeat(64)}`
  const binding: Binding = { accountId, modelId: 'haiku', modelOptions: {} }
  // textos-ok: dado simulado do Hub (fixture opt-in da prévia), não é rótulo da tela
  const definitions = [['vision', 'Visão'], ['context_compression', 'Compactar contexto'], ['message_triage', 'Mensagens durante o trabalho'], ['skills_hub', 'Hub de skills'], ['skill_selection', 'Selecionar skills'], ['approval_judge', 'Aprovações'], ['mcp_routing', 'Roteamento de ferramentas MCP'], ['memory_consolidation', 'Organizar memória'], ['bot_naming', 'Nomear bot'], ['bot_draft', 'Rascunho de novo bot'], ['voice_live', 'Conversa por voz'], ['speech_to_text', 'Ditado'], ['text_to_speech', 'Ler respostas']]
  const data: FunctionSnapshot = {
    document: { schemaVersion: 1, revision: 1, functions: {} }, accountsIssue: null, execution: 'not_implemented',
    accounts: [{ id: accountId, label: 'Claude de exemplo', backend: 'claude', state: 'connected', billing: 'subscription', models: [{ id: 'haiku', options: [{ id: 'reasoning_effort', values: ['low', 'high', 'max'], default: null }] }] }],
    functions: definitions.map(([id, name]) => {
      const managed = ['voice_live', 'speech_to_text', 'text_to_speech'].includes(id!)
      const selection: Selection = managed ? { mode: 'managedInVoice' } : id === 'memory_consolidation' ? { mode: 'custom', ...binding } : { mode: 'automatic', accountId, binding }
      return { definition: { id: id!, name: name!, managedInVoice: managed, recipe: 'Receita do computador de exemplo' }, selection, resolution: { status: 'unavailable', unavailable: { reason: 'capabilities_unconfirmed' } } }
    }),
  }
  for (const row of data.functions) data.document!.functions[row.definition.id] = row.selection!
  let spaceAvailable = false
  let historyAvailable = false
  let history: unknown = []
  const spaces: SpaceSnapshot = { execution: 'awaiting_executor', selections: [{ spaceId: 'demo-space', spaceName: 'Espaço de exemplo', spaceRevision: 1, functionId: 'memory_consolidation', revision: 0, state: 'not_authorized', current: { functionRevision: 1, accountId, accountRevision: 1, accountLabel: 'Claude de exemplo', modelId: 'haiku' }, audience: 'owner_private', execution: 'awaiting_executor' }] }
  host.call = async (method, body) => {
    if (!['functionModels', 'saveFunctionModel', 'checkFunctionModel', 'backgroundSpaceSelections', 'saveBackgroundSpaceSelection', 'backgroundHistory'].includes(method)) return original(method, body)
    requests.push({ method, body: structuredClone(body) })
    if (method === 'functionModels') return structuredClone(data)
    if (method === 'checkFunctionModel') return data.functions.find((r) => r.definition.id === body.functionId)!.resolution
    if (method === 'backgroundSpaceSelections') { if (!spaceAvailable) throw new Error('fixture: RPC ausente'); return structuredClone(spaces) }
    if (method === 'backgroundHistory') { if (!historyAvailable) throw new Error('fixture: histórico indisponível'); return structuredClone(history) }
    if (method === 'saveFunctionModel') {
      if (body.expectedRevision !== data.document!.revision) throw new Error('revision_changed')
      const selection = body.selection as Selection
      if (selection.mode === 'custom') data.document!.functions[String(body.functionId)] = structuredClone(selection)
      const row = data.functions.find((r) => r.definition.id === body.functionId)!
      row.selection = data.document!.functions[String(body.functionId)]!
      data.document!.revision++
      if (spaces.selections[0]!.state === 'configured') spaces.selections[0]!.state = 'stale'
      const memory = data.document!.functions.memory_consolidation
      const selected = memory?.mode === 'custom' ? memory : memory?.mode === 'automatic' ? memory.binding : null
      const account = selected && data.accounts.find((a) => a.id === selected.accountId)
      spaces.selections[0]!.current = selected && account ? { functionRevision: data.document!.revision, accountId: selected.accountId, accountRevision: 1, accountLabel: account.label, modelId: selected.modelId } : null
      host.emit({ type: 'functionModelsChanged', revision: data.document!.revision })
      return { document: structuredClone(data.document), execution: 'not_implemented' }
    }
    const row = spaces.selections[0]!
    if (body.expectedRevision !== row.revision || body.expectedSpaceRevision !== row.spaceRevision) throw new Error('revision_changed')
    row.state = body.enabled ? 'configured' : 'not_authorized'; row.revision++
    host.emit({ type: 'backgroundSpaceSelectionsChanged', spaceId: row.spaceId })
    return structuredClone(spaces)
  }
  return { data, spaces, requests, setSpaceAvailable(value: boolean) { spaceAvailable = value; host.emit({ type: 'backgroundSpaceSelectionsChanged', spaceId: 'demo-space' }) },
    setHistory(value: unknown) { history = value; historyAvailable = true; host.emit({ type: 'backgroundHistoryChanged' }) },
    setHistoryAvailable(value: boolean) { historyAvailable = value; host.emit({ type: 'backgroundHistoryChanged' }) },
    restore() { host.call = original } }
}
