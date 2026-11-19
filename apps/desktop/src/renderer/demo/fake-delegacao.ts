// D371 (T1), só na demo: as regras de delegação e um quadro de Equipes com Marketing, em memória, no formato do
// Hub (`host/src/fork/delegacao`, `host/src/fork/times`). Dados fictícios; a versão segue a mesma regra de CAS.
const PESSOAL = '00000000-0000-4000-8000-000000000001'
const MARKETING = 'a1b2c3d4-0000-4000-8000-00000000000a'

const GERAL = [
  'Coordenadores: Opus 5.5 high ou Sol high.',
  'Desenvolvimento: Opus 5.5 medium ou Sol medium. Interface fica com o Claude.',
  'Tarefas simples (busca, commit, ferramentas): Haiku xhigh ou Luna high.',
  'Bug travado: Fable high ou Astra xhigh, só quando os outros não resolveram.',
].join('\n')
const DA_MARKETING = 'Texto de campanha: peça à Prosa e mande o Crivo revisar antes de publicar.\nPesquisa de concorrentes: Haiku xhigh basta.'

export class FakeDelegacao {
  version = 2
  general = GERAL
  teams: Record<string, string> = { [MARKETING]: DA_MARKETING }
  private receipts = new Map<string, number>()

  atende(method: string) { return method === 'donnaDelegationRules' || method === 'donnaDelegationRulesSave' || method === 'donnaTimes' }

  call(method: string, b: Record<string, unknown>, bots: { id: string; name: string; avatarColor: string; avatarShape: string }[], primary: string | null, fail: (m: string) => unknown): unknown {
    if (method === 'donnaTimes') return this.times(bots, primary)
    const snapshot = () => ({ spaceId: PESSOAL, version: this.version, limit: 4000, general: this.general, teams: { ...this.teams } })
    if (method === 'donnaDelegationRules') return snapshot()
    const id = String(b.clientRequestId)
    if (!this.receipts.has(id)) {
      if (b.expectedVersion !== this.version) throw fail('delegation_conflict: as regras mudaram; releia antes de salvar.')
      const text = String(b.text ?? '').trim()
      const team = typeof b.teamId === 'string' ? b.teamId : null
      if (team) this.teams[team] = text
      else this.general = text
      this.version++
      this.receipts.set(id, this.version)
    }
    return { snapshot: snapshot(), receipt: { clientRequestId: id, version: this.receipts.get(id) } }
  }

  private times(bots: { id: string; name: string; avatarColor: string; avatarShape: string }[], primary: string | null) {
    const chefe = 'prosa', membro = 'crivo'
    return {
      version: 1, spaceId: PESSOAL, revision: 4, catalogVersion: 'demo', primaryBotId: primary,
      bots: bots.map((b) => ({ id: b.id, name: b.name, spaceId: PESSOAL, title: '', avatarColor: b.avatarColor, avatarShape: b.avatarShape, hidden: false })),
      nodes: [{ id: MARKETING, name: 'Marketing', parentId: null, managerBotId: chefe, position: 0 }],
      members: bots.map((b) => ({
        botId: b.id,
        teamId: b.id === chefe || b.id === membro ? MARKETING : null,
        parentBotId: null,
        role: b.id === chefe || b.id === primary ? 'manager' : 'executor',
        reportsTo: b.id === membro ? { kind: 'bot', botId: chefe } : { kind: 'owner' },
        ownerContact: b.id === membro ? 'on_request' : 'reports',
      })),
      routes: [],
    }
  }
}
