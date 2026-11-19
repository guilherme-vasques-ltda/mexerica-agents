import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { TimesGraph, TimesSnapshot } from '../../../../shared/fork-times.ts'
import { gravarEquipe, mudancaDaEquipe, opcoesDeEquipe, type SessaoEquipe } from './equipe.ts'

const membro = (botId: string, teamId: string | null, role: 'manager' | 'executor' = 'executor') =>
  ({ botId, teamId, parentBotId: null, role, reportsTo: { kind: 'owner' as const }, ownerContact: 'reports' as const })
const bot = (id: string, name: string) => ({ id, name, spaceId: 'personal', title: '', avatarColor: '', avatarShape: '', hidden: false })

function quadro(extra: string[] = []): TimesSnapshot {
  return {
    version: 1, spaceId: 'personal', revision: 3, catalogVersion: 'c1', primaryBotId: 'donna',
    nodes: [
      { id: 'mkt', name: 'Marketing', parentId: null, managerBotId: 'hype', position: 0 },
      { id: 'cont', name: 'Conteúdo', parentId: 'mkt', managerBotId: null, position: 0 },
    ],
    members: [membro('donna', null, 'manager'), membro('hype', 'mkt', 'manager'), membro('prosa', 'mkt', 'manager'), membro('feed', 'mkt'), ...extra.map((id) => membro(id, null))],
    routes: [],
    bots: [bot('donna', 'Donna'), bot('hype', 'Hype'), bot('prosa', 'Prosa'), bot('feed', 'Feed'), ...extra.map((id) => bot(id, id))],
  }
}

/** Uma sessão de mentira com o mesmo contrato da TimesSession: guarda o que foi conferido e gravado. */
function sessao(opcoes: { snapshot?: TimesSnapshot | null; propor?: boolean; gravar?: boolean | 'unknown' | 'conflict'; falha?: string } = {}) {
  const s = {
    phase: { kind: opcoes.falha ? 'error' : 'ready', failure: opcoes.falha } as { kind: string; failure?: string },
    check: { kind: 'idle' },
    save: { kind: 'idle' },
    proposto: null as TimesGraph | null,
    gravacoes: 0,
    reenvios: 0,
    async sync() { return opcoes.snapshot === undefined ? quadro(['novo']) : opcoes.snapshot },
    async propose(_: string, change: (g: TimesGraph) => TimesGraph) {
      if (opcoes.propor === false) { s.check = { kind: 'refused' }; return false }
      s.proposto = change(s.proposto ?? quadro(['novo']))
      return true
    },
    async commit() {
      s.gravacoes++
      if (opcoes.gravar === 'unknown' || opcoes.gravar === 'conflict') { s.save = { kind: opcoes.gravar }; return false }
      return opcoes.gravar !== false
    },
    async retrySave() { s.reenvios++; s.save = { kind: 'idle' }; return true },
    discard() { s.proposto = null },
  }
  return s satisfies SessaoEquipe
}

test('equipes e chefes que o quadro já tem, com caminho; a Donna nunca é chefe', () => {
  const opcoes = opcoesDeEquipe(quadro())
  assert.deepEqual(opcoes.map((o) => o.nome), ['Marketing', 'Marketing › Conteúdo'])
  assert.deepEqual(opcoes[0]!.chefes.map((c) => c.nome), ['Hype', 'Prosa'], 'o chefe da área primeiro')
  assert.equal(opcoes[0]!.chefePadrao, 'hype')
  assert.deepEqual(opcoes[1]!.chefes, [])
  const comDonna = { ...quadro(), nodes: [{ id: 'x', name: 'X', parentId: null, managerBotId: 'donna', position: 0 }], members: [membro('donna', 'x', 'manager')] }
  assert.deepEqual(opcoesDeEquipe(comDonna)[0]!.chefes, [], 'D366: a principal não aparece como chefe')
})

test('dois passos: o bot novo entra na equipe e reporta ao chefe; o resto do quadro não muda', async () => {
  const s = sessao()
  assert.deepEqual(await gravarEquipe(s, 'novo', { areaId: 'mkt', chefeId: 'hype' }), { ok: true })
  assert.equal(s.gravacoes, 1)
  const novo = s.proposto!.members.find((m) => m.botId === 'novo')!
  assert.deepEqual([novo.teamId, novo.parentBotId, novo.reportsTo], ['mkt', null, { kind: 'bot', botId: 'hype' }])
  const antes = quadro(['novo'])
  assert.deepEqual(s.proposto!.members.filter((m) => m.botId !== 'novo'), antes.members.filter((m) => m.botId !== 'novo'))
  assert.deepEqual(s.proposto!.nodes, antes.nodes)
})

test('D366/D368: a principal nunca é movida pela criação', () => {
  assert.throws(() => mudancaDaEquipe('donna', { areaId: 'mkt', chefeId: 'hype' }, 'donna')(quadro()), /primary_protected/)
})

test('segundo passo falha: código estável, sem desfazer o bot', async () => {
  assert.deepEqual(await gravarEquipe(sessao({ snapshot: null }), 'novo', { areaId: 'mkt', chefeId: null }), { ok: false, codigo: 'sem_leitura' })
  assert.deepEqual(await gravarEquipe(sessao({ snapshot: null, falha: 'owner' }), 'novo', { areaId: 'mkt', chefeId: null }), { ok: false, codigo: 'sem_dono' })
  assert.deepEqual(await gravarEquipe(sessao({ snapshot: quadro() }), 'novo', { areaId: 'mkt', chefeId: null }), { ok: false, codigo: 'bot_fora_do_quadro' })
  assert.deepEqual(await gravarEquipe(sessao(), 'novo', { areaId: 'sumiu', chefeId: null }), { ok: false, codigo: 'equipe_sumiu' })
  assert.deepEqual(await gravarEquipe(sessao(), 'novo', { areaId: 'mkt', chefeId: 'feed' }), { ok: false, codigo: 'chefe_mudou' }, 'Feed não é gerente')
  assert.deepEqual(await gravarEquipe(sessao({ propor: false }), 'novo', { areaId: 'mkt', chefeId: null }), { ok: false, codigo: 'recusada' })
  assert.deepEqual(await gravarEquipe(sessao({ gravar: 'conflict' }), 'novo', { areaId: 'mkt', chefeId: null }), { ok: false, codigo: 'conflito' })
})

test('Tentar de novo depois de gravação sem resposta reenvia a mesma intenção, sem nova gravação', async () => {
  const s = sessao({ gravar: 'unknown' })
  assert.deepEqual(await gravarEquipe(s, 'novo', { areaId: 'mkt', chefeId: 'hype' }), { ok: false, codigo: 'sem_resposta' })
  assert.deepEqual(await gravarEquipe(s, 'novo', { areaId: 'mkt', chefeId: 'hype' }), { ok: true })
  assert.deepEqual([s.gravacoes, s.reenvios], [1, 1])
})

test('depois da falha, Tentar de novo usa o pedido guardado e a tela fica travada', async () => {
  const { destinoTravado, tentarDeNovo } = await import('./equipe.ts')
  const s = sessao()
  const pos = { id: 'novo', nome: 'Feed', spaceId: null, equipe: { escolha: { areaId: 'mkt', chefeId: 'hype' }, nome: 'Marketing' }, fase: 'falhou' as const, codigo: 'conflito' as const }
  assert.equal(destinoTravado(pos), true, 'equipe, chefe e Space travados durante a pós-criação')
  assert.equal(destinoTravado(null), false)
  // Mesmo que a tela mostre outra escolha (ou Sem equipe), a nova tentativa grava a equipe e o chefe pedidos.
  assert.deepEqual(await tentarDeNovo(s, pos, null), { ok: true })
  const novo = s.proposto!.members.find((m) => m.botId === 'novo')!
  assert.deepEqual([novo.teamId, novo.reportsTo], ['mkt', { kind: 'bot', botId: 'hype' }])
  // Sessão de outro lugar nunca recebe a gravação.
  const outra = sessao()
  assert.deepEqual(await tentarDeNovo(outra, pos, 'loja'), { ok: false, codigo: 'sem_leitura' })
  assert.equal(outra.gravacoes, 0)
})
