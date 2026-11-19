// Dados de teste: fichas inventadas no formato do Hub (nada de serviço real). Só os testes importam.
import type { Catalogo, Conta, Grupo, Servico, TipoConexao } from './tipos.ts'

export const grupo = (id: string, extra: Partial<Grupo> = {}): Grupo => ({ id, rotulo: id, escopos: [], url: null, ferramentas: [], padrao: false, disponivel: true, ...extra })

export const tipo = (grupos: Grupo[], extra: Partial<TipoConexao> = {}): TipoConexao => ({
  id: 'x.oficial_oauth', tipo: 'oficial_oauth', rotulo: 'Oficial', mantidoPor: 'X', url: 'https://x.example/mcp',
  auth: { modo: 'oauth', registro: 'dcr' }, grupos, perigosas: [], recomendado: true, disponivel: true, motivoIndisponivel: null, ...extra,
})

export const servico = (id: string, nome: string, extra: Partial<Servico> = {}): Servico => ({
  id, marca: id, nome, categoria: 'Projetos', descricao: `${nome} de teste`, exemplos: [], links: { privacidade: 'https://x.example/p', termos: 'https://x.example/t' },
  conferidaEm: '2026-11-17', fontes: [], aviso: null, tipos: [tipo([grupo('ler', { padrao: true })])], skills: [], emBreve: null,
  conectavel: true, conferenciaVigente: true, diasDesdeConferencia: 0, contas: [], ...extra,
})

export const conta = (id: string, name: string, servicoId: string | null, extra: Partial<Conta> = {}): Conta => ({
  id, name, servico: servicoId, tipo: servicoId ? 'oficial_oauth' : null, liberacoes: ['ler'], space: 'pessoal', identidade: null,
  auth: 'signedIn', origem: servicoId ? 'catalogo' : 'usuario', naoVerificado: servicoId === null, bots: [], ...extra,
})

export const catalogo = (servicos: Servico[], categorias?: string[]): Catalogo => ({ versao: 3, prazoConferenciaDias: 90, categorias, hoje: '2026-11-17', spaceId: 'pessoal', servicos })
