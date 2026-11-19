import assert from 'node:assert/strict'
import { test } from 'node:test'
import { configFromRole, DEFAULT_CONFIG, ROLES } from './fork-avatar-config.ts'
import { arteDoAvatar, configDaFrutinha, FRUTINHAS, ROSTO_PRINCIPAL } from './fork-frutinhas.ts'

// D478: tabela selo → frutinha nos 30 presets (o mapeamento aprovado, função por função).
const ESPERADO: Record<string, string | null> = {
  donna: null, chief: 'ceo', pm: 'ceo', calendar: 'assistente', notes: 'assistente', research: 'pesquisa', travel: 'assistente',
  translator: 'assistente', dev: 'developer', qa: 'developer', ecommerce: 'vendas', amazon: 'vendas', importacao: 'compras',
  compras: 'compras', estoque: 'compras', suporte: 'suporte', marketing: 'marketing', metaads: 'marketing', social: 'marketing',
  escritor: 'marketing', seo: 'marketing', youtuber: 'marketing', designer: 'marketing', vendas: 'vendas', sdr: 'vendas',
  financeiro: 'financeiro', contabil: 'financeiro', bookkeeper: 'financeiro', rh: 'rh', psicologa: 'rh',
}

test('cada um dos 30 presets vira a frutinha da sua função; o do principal segue o ponto único', () => {
  assert.equal(ROLES.length, 30)
  for (const role of ROLES) assert.equal(arteDoAvatar(configFromRole(role), 'atual'), ESPERADO[role.id], role.id)
})

test('selo sem função fica robô; robô editado à mão com selo de função vira a frutinha', () => {
  for (const badge of ['none', 'clipboard', 'mail', 'bolt', 'rocket', 'bulb', 'mic'] as const) assert.equal(arteDoAvatar({ badge }), null, badge)
  // Lista fechada (Kodus #555): selo desconhecido ou chave do protótipo cai no robô.
  for (const badge of ['constructor', 'toString', '__proto__', 'hasOwnProperty', 'moeda', '']) assert.equal(arteDoAvatar({ badge } as never), null, badge)
  const feitoAMao = { ...DEFAULT_CONFIG, shape: 'gato' as const, color: '#123456', eye: 'pixel' as const, head: 'beret' as const, badge: 'coin' as const }
  assert.equal(arteDoAvatar(feitoAMao), 'financeiro')
})

test('D486: o rosto do Berga (selo sparkle) nas duas posições do ponto único', () => {
  const berga = configFromRole(ROLES.find((r) => r.id === 'donna')!)
  assert.equal(ROSTO_PRINCIPAL, 'mascote', 'D486: o Berga usa a mascote')
  assert.equal(arteDoAvatar(berga), 'mexerica')
  assert.equal(arteDoAvatar(berga, 'atual'), null)
  assert.equal(arteDoAvatar(berga, 'mascote'), 'mexerica')
  // A posição do Berga não muda nenhum outro rosto.
  for (const role of ROLES.filter((r) => r.id !== 'donna')) assert.equal(arteDoAvatar(configFromRole(role), 'mascote'), ESPERADO[role.id])
})

test('escolher uma frutinha no editor aplica um preset que volta a mostrar a mesma frutinha', () => {
  for (const f of FRUTINHAS) assert.equal(arteDoAvatar(configDaFrutinha(f)), f, f)
})
