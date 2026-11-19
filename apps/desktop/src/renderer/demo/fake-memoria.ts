// D389, só na demo: o modelo da memória (a função `memory_consolidation` do Bot de fundo), o modo de aprender e
// duas sugestões, em memória, no formato do Hub (`host/src/fork/memory_learning.rs`, `memory_auto/suggestions.rs`).
// Dados fictícios; aprovar e descartar só mexem nesta lista.
type Fail = (message: string) => unknown

const CONTA = { id: 'demo-claude', label: 'Assinatura pessoal', backend: 'claude', state: 'connected', billing: 'subscription',
  models: [{ id: 'claude-haiku-5-5', options: [] }, { id: 'claude-sonnet-5-5', options: [] }, { id: 'claude-opus-5-5', options: [] }] }
const ORIGEM = { schema: 1, kind: 'user', sources: [], external: false }
// MX-8: "Sobre você", a nota global com cinco seções (views/fork/memoria-ajustes/sobre-voce.ts). `?sobrevoce=cheio` já começa preenchida.
const SOBRE_VOCE = '# Sobre você\n\n## Quem é\nAna Ribeiro, 38 anos, Curitiba (UTC−3). Casada com Léo; duas filhas, Bia e Lu.\n\n## Como pensa\nDecide com números na mão. Prefere uma recomendação clara a uma lista de opções.\n\n## O que faz\nFundadora de uma loja on-line de cafés especiais. Segunda é dia de fechamento.\n\n## Como fala\nPortuguês, direto, frases curtas. Respostas de até cinco linhas.\n\n## O que não fazer\nNão marcar nada antes das 10h. Não usar emoji. Não mandar mensagem a cliente sem o meu OK.\n'
const sobreVoceCheio = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('sobrevoce') === 'cheio'

export class FakeMemoria {
  mode: 'auto' | 'suggest' | 'off' = 'suggest'
  revision = 3
  selection: Record<string, unknown> = { mode: 'automatic', accountId: 'demo-claude', binding: { accountId: 'demo-claude', modelId: 'claude-haiku-5-5', modelOptions: { reasoning_effort: 'low' } } }
  profile: { content: string | null; revision: number } = { content: sobreVoceCheio() ? SOBRE_VOCE : null, revision: 1 }
  suggestions = [
    { id: 'demo-sugestao-1', action: 'remember', text: 'Prefere reuniões depois das 10h.', file: null, shared: true, origin: ORIGEM, nature: 'said', createdAt: Date.now() - 3_600_000 },
    { id: 'demo-sugestao-2', action: 'revise', text: 'Mora em Curitiba desde 2024.', file: null, shared: false, origin: { ...ORIGEM, kind: 'conversation' }, nature: 'inferred', createdAt: Date.now() - 600_000 },
  ]

  atende(method: string) {
    return ['memoryLearning', 'saveMemoryLearning', 'functionModels', 'saveFunctionModel', 'approveMemorySuggestion', 'discardMemorySuggestion', 'memoryProfile', 'saveMemoryProfile'].includes(method)
  }

  /** Acrescenta o modo e as sugestões à Memória do bot da demo. */
  view(base: Record<string, unknown>) { return { ...base, learning: this.mode, suggestions: this.suggestions } }

  call(method: string, b: Record<string, unknown>, fail: Fail): unknown {
    const state = () => ({ mode: this.mode, pending: this.suggestions.length })
    switch (method) {
      case 'memoryLearning': return state()
      case 'memoryProfile': return this.profileView()
      case 'saveMemoryProfile':
        if (typeof location !== 'undefined' && new URLSearchParams(location.search).get('falha') === 'gravar') throw fail('Hub demo: tempo esgotado')
        if (b.base !== String(this.profile.revision)) throw fail('memory_profile_conflict')
        // #698: o Hub recusa o que parece segredo, sem repetir o trecho.
        if (typeof b.content === 'string' && /senha|password|token|api[_ -]?key/i.test(b.content)) throw fail('secret_detected')
        if (b.content !== null && (typeof b.content !== 'string' || !b.content.trim())) throw fail('invalid_field: nota vazia.')
        this.profile = { content: b.content as string | null, revision: this.profile.revision + 1 }
        return this.profileView()
      case 'saveMemoryLearning':
        if (!['auto', 'suggest', 'off'].includes(String(b.mode))) throw fail('invalid_field: escolha de memória inválida.')
        this.mode = b.mode as typeof this.mode
        return state()
      case 'functionModels': return this.functions()
      case 'saveFunctionModel':
        if (b.expectedRevision !== this.revision) throw fail('revision_conflict')
        this.selection = b.selection as Record<string, unknown>
        this.revision++
        return { document: this.document(), execution: 'partial' }
      default: {
        const found = this.suggestions.find((s) => s.id === b.id)
        if (!found) throw fail('memory_suggestion_not_found')
        this.suggestions = this.suggestions.filter((s) => s !== found)
        return method === 'approveMemorySuggestion' ? { status: 'approved', receipt: null } : { status: 'discarded' }
      }
    }
  }

  /** MX-8: lembranças compartilhadas sobre você (camada global), para a seção "Compartilhado sobre você" do bot. */
  compartilhadas() {
    if (!sobreVoceCheio() && !(typeof location !== 'undefined' && new URLSearchParams(location.search).has('sobrevoce'))) return []
    const at = Date.now() - 86_400_000
    return ['Prefere reuniões depois das 10h.', 'Toma café sem açúcar.'].map((text, i) => ({ id: 900 + i, bot: null, state: 'active', text, author: 'user', authorBot: null, pinned: false, evidence: [], createdAt: at, updatedAt: at, origin: ORIGEM, nature: 'said' }))
  }

  private profileView() { return { content: this.profile.content, revision: String(this.profile.revision), loose: this.compartilhadas().length } }

  private document() { return { schemaVersion: 1, revision: this.revision, functions: { memory_consolidation: this.selection } } }

  private functions() {
    return { document: this.document(), accounts: [CONTA], accountsIssue: null, execution: 'partial', approvalPolicy: 'managed_by_rules_starts_observing',
      functions: [{ definition: { id: 'memory_consolidation', name: 'Organizar memória', managedInVoice: false, recipe: 'haiku_low' }, selection: this.selection, consumer: 'background_worker',
        resolution: { status: 'available', resolved: { effective: { accountId: 'demo-claude', modelId: 'claude-haiku-5-5', modelOptions: { reasoning_effort: 'low' } } } } }] }
  }
}
