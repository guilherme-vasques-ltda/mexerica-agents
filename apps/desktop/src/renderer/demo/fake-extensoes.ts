import type { InstalledSkillV2 } from '@shared/fork-skills'

// Revisão do app (Entrega 2), só na demo e só com `?extensoes` na URL: conectores, credenciais e uso de skills de
// exemplo para os prints da proposta. Nada de conta, rede ou serviço real: nomes e endereços são inventados.

export const extensoesDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('extensoes')

/** A resposta de `connectors`: três MCP remotos de contas, um MCP remoto e um local. */
export const conectoresDemo = () => ({
  items: [
    { id: 'demo-pagamentos', name: 'Pagamentos', description: 'Cobranças e assinaturas', kind: 'remote', url: 'https://mcp.example.invalid/conta', keys: ['PAGAMENTOS_API_KEY'] },
    { id: 'demo-agenda', name: 'Agenda', description: 'Compromissos e convites', kind: 'remote', url: 'https://mcp.example.invalid/conta', keys: [] },
    { id: 'demo-hospedagem', name: 'Hospedagem', description: 'Domínios e servidores', kind: 'remote', url: 'https://mcp.example.invalid/conta', keys: ['HOSPEDAGEM_API_TOKEN'] },
    { id: 'demo-tarefas', name: 'Tarefas do time', description: 'Pendências e prazos', kind: 'remote', url: 'https://mcp.example.invalid/tarefas', keys: ['Authorization'], auth: 'signedIn' },
    { id: 'demo-docs', name: 'Documentos (demo)', description: 'Conector sintético', kind: 'local', command: 'npx mcp-documentos', keys: [] },
  ],
})

export const loginsDemo = () => ({
  items: [
    { id: 'demo-login', site: 'example.invalid', username: 'demo', label: 'Conta de teste (demo)' },
    { id: 'demo-login-2', site: 'painel.example.invalid', username: 'financeiro@example.invalid', label: '' },
  ],
})

/** Quais bots da história têm cada coisa ligada (para a confirmação de remover e as Ferramentas do bot). */
export const LIGADOS: Record<string, { skills: string[]; connectors: string[]; logins: string[] }> = {
  sprint: { skills: ['find-skills', 'demo-skill'], connectors: ['demo-pagamentos', 'demo-agenda', 'demo-tarefas'], logins: ['demo-login'] },
  byte: { skills: ['find-skills'], connectors: ['demo-hospedagem'], logins: [] },
  crivo: { skills: ['find-skills', 'pdf'], connectors: [], logins: [] },
}

/**
 * PRECISA DO HUB: último uso por skill (data e contagem, sem texto de conversa). O Hub de hoje não guarda nem manda
 * isso; aqui é só o desenho. `pdf` fica sem o campo de propósito, para mostrar a linha de quem nunca foi usada.
 */
const USO: Record<string, { diasAtras: number; count: number }> = {
  'find-skills': { diasAtras: 0, count: 37 },
  'content-strategy-v0': { diasAtras: 12, count: 4 },
  'demo-skill': { diasAtras: 2, count: 9 },
}
export function comUso(skills: InstalledSkillV2[], agora = Date.now()): InstalledSkillV2[] {
  return skills.map((s) => {
    const u = USO[s.id]
    return { ...s, usage: u ? { lastUsedAt: agora - u.diasAtras * 86_400_000 - 2 * 3_600_000, count: u.count } : { lastUsedAt: null, count: 0 } }
  })
}

// MX-84 (Bot → Ferramentas em abas), só na demo e só com `?extensoes&abas=muitos|poucos`. Imita o Hub real: uma linha
// por CONTA conectada, id `conta-<app>-<conta>` e o rótulo já dentro do nome ("Pagamentos · Loja A"). As listas são do
// computador, iguais para todo bot: "muitos" é um computador com 23 conectores; "poucos", um com 4.
type ModoAbas = 'muitos' | 'poucos' | 'vazio'
export const abasDemo = (): ModoAbas | null => {
  if (!extensoesDemo()) return null
  const v = new URLSearchParams(location.search).get('abas')
  return v === 'muitos' || v === 'poucos' || v === 'vazio' ? v : null
}

const conta = (app: string, id: string, nome: string, description: string) => ({ id: `conta-${app}-${id}`, name: nome, description, kind: 'remote', url: `https://mcp.example.invalid/${app}`, keys: [] as string[] })
const remoto = (id: string, name: string, description: string, keys: string[] = []) => ({ id: `demo-${id}`, name, description, kind: 'remote', url: `https://mcp.example.invalid/${id}`, keys, auth: 'signedIn' })

const CONECTORES_ABAS = [
  conta('pagamentos', 'ca_lojaa', 'Pagamentos · Loja A', 'Cobranças e assinaturas'),
  conta('pagamentos', 'ca_lojab', 'Pagamentos · Loja B', 'Cobranças e assinaturas'),
  conta('agenda', 'ca_pessoal', 'Agenda · Pessoal', 'Compromissos e convites'),
  conta('agenda', 'ca_trabalho', 'Agenda · Trabalho', 'Compromissos e convites'),
  conta('hospedagem', 'ca_1', 'Hospedagem', 'Domínios e servidores'),
  conta('repositorios', 'ca_1', 'Repositórios', 'Código, pendências e revisões'),
  conta('cadernos', 'ca_1', 'Cadernos', 'Páginas e anotações'),
  remoto('tarefas', 'Tarefas do time', 'Pendências e prazos', ['Authorization']),
  { id: 'demo-docs', name: 'Documentos (demo)', description: '', kind: 'local', command: 'npx mcp-documentos', keys: [] },
  conta('recados', 'ca_1', 'Recados', 'Mensagens e canais'),
  conta('correio', 'ca_1', 'Correio', 'E-mails e rascunhos'),
  conta('arquivos', 'ca_1', 'Arquivos', 'Pastas e documentos'),
  conta('chamados', 'ca_1', 'Chamados', 'Projetos e pedidos'),
  conta('clientes', 'ca_1', 'Clientes', 'Contatos e negócios'),
  remoto('prototipos', 'Protótipos', ''),
  // Daqui para baixo é a página 2.
  conta('comunidade', 'ca_1', 'Comunidade', 'Grupos e mensagens'),
  conta('pastas', 'ca_1', 'Pastas compartilhadas', 'Arquivos do time'),
  conta('lojavirtual', 'ca_1', 'Loja virtual', 'Pedidos e produtos'),
  remoto('tabelas', 'Tabelas', 'Tabelas e registros', ['Authorization']),
  remoto('quadros', 'Quadros', 'Quadros e cartões'),
  remoto('projetos', 'Projetos', ''),
  remoto('atendimento', 'Atendimento', 'Conversas com clientes'),
  { id: 'demo-banco-local', name: 'Banco local', description: '', kind: 'local', command: 'npx mcp-banco --somente-leitura', keys: ['DATABASE_URL'] },
]

export const conectoresAbasDemo = (modo: ModoAbas) => ({
  items: modo === 'vazio' ? [] : modo === 'muitos' ? CONECTORES_ABAS : [CONECTORES_ABAS[0]!, CONECTORES_ABAS[1]!, CONECTORES_ABAS[2]!, CONECTORES_ABAS[7]!],
})

const SITES = ['banco', 'contabilidade', 'fornecedor', 'prefeitura', 'correios', 'hospedagem', 'dominios', 'anuncios', 'loja', 'frete', 'suporte', 'folha', 'notas', 'estoque', 'viagens']
export const loginsAbasDemo = (modo: ModoAbas) => ({
  items: modo === 'vazio' ? [] : [
    ...loginsDemo().items,
    ...(modo === 'muitos' ? SITES.map((s, i) => ({ id: `demo-login-${s}`, site: `${s}.example.invalid`, username: i % 3 ? `time@${s}.example.invalid` : 'demo', label: i % 2 ? '' : `${s[0]!.toUpperCase()}${s.slice(1)} (demo)` })) : []),
  ],
})

const SKILLS_ABAS = ['Atas de reunião', 'Briefing de campanha', 'Checklist de lançamento', 'Conferir contrato', 'Diário de bordo', 'E-mail de cobrança', 'Fechamento do mês', 'Guia de estilo', 'Importar planilha', 'Legendas de vídeo', 'Mapa de riscos', 'Notas de versão', 'Pauta semanal', 'Proposta comercial', 'Resumo de artigo', 'Roteiro de entrevista']
// Metade com descrição longa: a linha mostra 2 linhas e o "ver mais" abre o resto ali mesmo.
const LONGA = 'Skill sintética da demo, com descrição longa de propósito. Lê o material enviado, separa decisões, responsáveis e prazos, confere datas e valores contra o que já foi combinado e devolve um texto pronto para mandar ao time, com os pontos em aberto listados no fim e o que falta confirmar marcado.'
/** Skills a mais na lista de instaladas (sintéticas, sem recibo), para a aba Skills passar de 15. */
export const skillsAbasDemo = (): InstalledSkillV2[] => SKILLS_ABAS.map((name, i) => ({
  id: `demo-abas-${i + 1}`, name: `${name} (demo)`, description: i % 2 ? 'Skill sintética da demo, para conferir busca e páginas.' : LONGA, source: '', path: `skills/demo-abas-${i + 1}`, receipt: null,
  installationId: `deadbeef-0000-4000-8000-0000000000ab:${i + 1}`, contentStatus: 'verified',
}))

/** O que o bot principal tem ligado na cena das abas: uma das duas contas de Pagamentos, e um conector que mora na página 2. */
export const LIGADOS_ABAS = {
  skills: ['find-skills', 'demo-skill', 'demo-abas-3'],
  connectors: ['conta-pagamentos-ca_lojaa', 'conta-agenda-ca_pessoal', 'demo-tarefas', 'conta-repositorios-ca_1', 'conta-cadernos-ca_1', 'conta-lojavirtual-ca_1'],
  logins: ['demo-login', 'demo-login-banco'],
}
