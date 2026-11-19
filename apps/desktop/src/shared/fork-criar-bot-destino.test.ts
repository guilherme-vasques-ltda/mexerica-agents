import assert from 'node:assert/strict'
import { test } from 'node:test'
import { contasDoMotor, corpoDaCriacao, escolherMotor, erroDaCriacao, escolhaDeConta, noSpace, spacesDaCriacao } from './fork-criar-bot-destino.ts'
import type { ManagedAccount } from './fork-engine-accounts.ts'
import type { SpacesSnapshot } from './fork-spaces.ts'

const base = { name: 'Crivo', backend: 'codex', title: 'QA', permission: 'ask', model: null }

test('o createBot leva conta e Space só quando escolhidos, e nada mais', () => {
  assert.deepEqual(corpoDaCriacao(base, { conta: null, spaceId: null }, 'personal'), base, 'padrão: o mesmo corpo da fatia 1')
  assert.deepEqual(corpoDaCriacao(base, { conta: null, spaceId: 'personal' }, 'personal'), base, 'Pessoal não manda spaceId')
  assert.deepEqual(corpoDaCriacao(base, { conta: 'acc-1', spaceId: 'loja' }, 'personal'),
    { ...base, forkEditor: { engineAccountId: 'acc-1' }, spaceId: 'loja' })
  assert.equal(noSpace({ conta: null, spaceId: 'loja' }, 'personal'), true)
  assert.equal(noSpace({ conta: null, spaceId: 'personal' }, 'personal'), false)
})

test('sem a posse de engine.accounts não há escolha de conta; motor sem conta também não', () => {
  assert.equal(escolhaDeConta(true, 'codex'), 'escolher')
  assert.equal(escolhaDeConta(false, 'claude'), 'sem-posse')
  assert.equal(escolhaDeConta(true, 'gemini'), 'motor-sem-conta')
})

test('contas do motor: só as gerenciadas do mesmo motor; só a conectada é escolhível', () => {
  const conta = (id: string, backend: 'codex' | 'claude', state: ManagedAccount['state'], origin: ManagedAccount['origin'] = 'managed') =>
    ({ id, label: id, brand: 'OpenAI', backend, billing: 'subscription', state, models: [], revision: 1, origin, botIds: [] }) as ManagedAccount
  const lista = contasDoMotor([conta('a', 'codex', 'connected'), conta('b', 'codex', 'disconnected'), conta('c', 'claude', 'connected'), conta('l', 'codex', 'connected', 'legacy')], 'codex')
  assert.deepEqual(lista.map((x) => [x.conta.id, x.escolhivel]), [['a', true], ['b', false]])
})

test('Spaces: Pessoal primeiro e os outros na ordem da faixa', () => {
  const s = (id: string, kind: 'personal' | 'space', position: number) => ({ id, kind, name: id, position }) as SpacesSnapshot['spaces'][number]
  const snap = { version: 1, personalSpaceId: 'p', revision: 1, spaces: [s('b', 'space', 2), s('p', 'personal', 0), s('a', 'space', 1)] } as SpacesSnapshot
  assert.deepEqual(spacesDaCriacao(snap).map((x) => x.id), ['p', 'a', 'b'])
  assert.deepEqual(spacesDaCriacao(null), [])
})

test('recusa do Hub vira código estável, sem eco da mensagem', () => {
  assert.equal(erroDaCriacao(new Error('remote_setup_unavailable: este aparelho não tem autorização vigente para gerenciar contas de IA.')), 'remote_setup_unavailable')
  assert.equal(erroDaCriacao(new Error('account_unavailable: reconecte a conta.')), 'account_unavailable')
  assert.equal(erroDaCriacao(new Error('space_backend_selection_required')), 'space_backend_selection_required')
  assert.equal(erroDaCriacao(new Error('/home/donna/.codex/auth.json: permission denied')), 'generico')
  assert.equal(erroDaCriacao(Object.assign(new Error('x'), { status: 0 })), 'offline')
})

test('motor já ativo não apaga a conta; trocar de motor limpa conta, modelo e opções juntos', () => {
  const draft = { ...base, model: 'gpt-5.3-codex', modelOptions: { reasoning_effort: 'high' } }
  const destino = { conta: 'acc-a', spaceId: 'loja' }
  assert.equal(escolherMotor(draft, destino, 'codex', 'codex'), null, 'clique no motor ativo não faz nada')
  const troca = escolherMotor(draft, destino, 'codex', 'claude')!
  assert.deepEqual(troca.destino, { conta: null, spaceId: 'loja' })
  assert.deepEqual([troca.draft.backend, troca.draft.model, troca.draft.modelOptions], ['claude', null, {}])
  // O corpo enviado depois da troca não leva o modelo nem a conta anteriores.
  const corpo = corpoDaCriacao(troca.draft, troca.destino, 'personal')
  assert.equal(corpo.forkEditor, undefined)
  assert.equal(corpo.model, null)
  assert.deepEqual(corpo.modelOptions, {})
})

test('falha ao ler os padrões: código estável e texto do catálogo nos seis idiomas, sem eco do Hub', async () => {
  const { falhaDosPadroes } = await import('./fork-criar-bot-destino.ts')
  const { CATALOGOS, traduzir } = await import('../renderer/lib/fork-i18n/index.ts')
  assert.equal(falhaDosPadroes(Object.assign(new Error('x'), { status: 0 })), 'offline')
  const vazamento = new Error('not_allowed: /home/donna/.donna/team.json: permission denied')
  assert.equal(falhaDosPadroes(vazamento), 'padroes')
  for (const idioma of Object.keys(CATALOGOS) as (keyof typeof CATALOGOS)[]) {
    for (const codigo of ['motores', 'offline', 'padroes'] as const) {
      const texto = traduzir(idioma, `botCreate.erro.${codigo}`)
      assert.ok(texto && !texto.includes('/home') && !texto.includes('not_allowed'), `${idioma} ${codigo}`)
    }
  }
})

test('D543: padrão do Space exige conta legada conectada e inequívoca do motor; nunca cai numa gerenciada', async () => {
  const { contaPadraoDoHub } = await import('./fork-criar-bot-destino.ts')
  const legacy = { id: 'padrao-do-hub-a', backend: 'codex', origin: 'legacy', state: 'connected' } as ManagedAccount
  const managed = { ...legacy, id: 'gerenciada', origin: 'managed' } as ManagedAccount
  assert.equal(contaPadraoDoHub([managed], 'codex'), null)
  assert.equal(contaPadraoDoHub([legacy], 'claude'), null)
  assert.equal(contaPadraoDoHub([{ ...legacy, state: 'unverified' }], 'codex'), null)
  assert.equal(contaPadraoDoHub([legacy, { ...legacy, id: 'ambigua' }], 'codex'), null)
  assert.equal(contaPadraoDoHub([managed, legacy], 'codex')?.id, legacy.id)
  const body = corpoDaCriacao(base, { conta: contaPadraoDoHub([legacy], 'codex')!.id, spaceId: 'trabalho' }, 'personal')
  assert.deepEqual(body.forkEditor, { engineAccountId: legacy.id }, 'confirmação manda vínculo explícito, não omissão')
})
