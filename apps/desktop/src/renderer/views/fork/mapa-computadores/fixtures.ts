// GUI-467: dados fictícios da demonstração (domínios reservados, sem endereço ou nome real). Nunca lidos
// de rede, Hub ou disco: a tela só os monta quando a demonstração está ligada.
import type { Computador } from './modelo.ts'

export function fixturas(agora: number): Computador[] {
  const ha = (minutos: number) => new Date(agora - minutos * 60_000).toISOString()
  return [
    {
      id: 'esta',
      nomeChave: 'mapaComputadores.nome.esta',
      papel: 'esta',
      posicao: { x: 50, y: 50 },
      fatos: { rota: 'nao_configurada', tunel: 'desconhecido', identidade: 'desconhecida', autorizacao: 'desconhecida' },
      amostra: { cpu: 38, memoria: 61, disco: 93, coletadaEm: ha(1) },
    },
    {
      id: 'demo-atlas',
      nomeChave: 'mapaComputadores.nome.atlas',
      papel: 'demo',
      posicao: { x: 18, y: 24 },
      fatos: { rota: 'configurada', tunel: 'parado', identidade: 'desconhecida', autorizacao: 'desconhecida' },
      amostra: { cpu: 12, memoria: 44, disco: 57, coletadaEm: ha(2) },
    },
    {
      id: 'demo-boreal',
      nomeChave: 'mapaComputadores.nome.boreal',
      papel: 'demo',
      posicao: { x: 80, y: 22 },
      fatos: { rota: 'configurada', tunel: 'em_uso', identidade: 'desconhecida', autorizacao: 'desconhecida' },
      amostra: { cpu: 70, memoria: 80, disco: 40, coletadaEm: ha(40) },
    },
    {
      id: 'demo-cedro',
      nomeChave: 'mapaComputadores.nome.cedro',
      papel: 'demo',
      posicao: { x: 22, y: 78 },
      fatos: { rota: 'configurada', tunel: 'em_uso', identidade: 'verificada', autorizacao: 'negada' },
      amostra: undefined,
    },
    {
      id: 'demo-dunas',
      nomeChave: 'mapaComputadores.nome.dunas',
      papel: 'demo',
      posicao: { x: 78, y: 76 },
      fatos: { rota: 'configurada', tunel: 'em_uso', identidade: 'verificada', autorizacao: 'confirmada' },
      amostra: { cpu: 140, memoria: 30, disco: 20, coletadaEm: ha(1) },
    },
    {
      id: 'demo-fjord',
      nomeChave: 'mapaComputadores.nome.fjord',
      papel: 'demo',
      posicao: { x: 50, y: 12 },
      fatos: { rota: 'desconhecida', tunel: 'desconhecido', identidade: 'desconhecida', autorizacao: 'desconhecida' },
      amostra: { cpu: 5, memoria: 20, disco: 10, coletadaEm: ha(3) },
    },
  ]
}
