// DON-907 C9a: `?troca=1` põe no chat de Byte a troca de Claude para Codex, como o Hub grava
// (host/src/agent/bot/fork_troca_motor.rs): a linha leiga com `modelSwitch` e o cartão da passagem (`engineSwitch`).
export const trocaDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('troca') === '1'

const de = { backend: 'claude', model: 'opus', label: 'Claude Code opus' }
const para = { backend: 'codex', model: 'gpt-6.1-sol', label: 'Codex gpt-6.1-sol' }

export const entradasTroca = () => [
  {
    id: 'troca-linha', kind: 'notice' as const, data: {
      text: `Modelo trocado de ${de.label} para ${para.label}. A conversa foi passada para o novo modelo.`, style: 'info', author: 'byte',
      modelSwitch: { from: de, to: para, kind: 'provider', carried: 'handoff', resentTokens: 18_400 },
    },
  },
  {
    id: 'troca-cartao', kind: 'notice' as const, data: {
      text: `O motor mudou de ${de.label} para ${para.label}. Sua próxima mensagem começa no motor novo levando este resumo.`, style: 'info', author: 'byte',
      handoff: { status: 'draft', reason: 'engineSwitch', text: '## Últimos pedidos\n- Add a weeklyTotals(runs) helper that sums distance and time per ISO week, with a test.\n\n## Plano aberto\n- [in_progress] Adding the invalid-date guard' },
    },
  },
]
