import { test } from 'node:test'
import assert from 'node:assert/strict'
import { localRefusal, type TimesBot, type TimesGraph } from '../../../../shared/fork-times.ts'
import type { BotDraft } from '../../../../shared/models.ts'
import { equipeMarketing, ExecucaoMarketing, executarMarketing, linhasMarketing, MARKETING, planoMarketing, rascunhoMarketing, type Dependencias } from './modelo-marketing.ts'
import { ROLES } from '../../../lib/donnabot-avatars/src/core/data.ts'
import { textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// D454 (fatia 9c) e D507: bloqueios e erros são `Frase`; aqui o texto pt-BR, sem as marcas de isolamento.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : '')

// D353 (Linear: DON-835): o modelo só compõe createBot e a mudança normal de Times. Chefe Hype, 6 membros reportando a
// ele, nada de poder novo, bots de Sem time reaproveitados, nada nasce antes de confirmar, falha no meio não grava.

const loose = (botId: string, role: 'manager' | 'executor' = 'executor') => ({ botId, teamId: null, parentBotId: null, role, reportsTo: { kind: 'owner' as const }, ownerContact: 'reports' as const })
const bot = (id: string, name: string, title: string, hidden = false): TimesBot => ({ id, name, spaceId: 'p', title, avatarColor: '', avatarShape: '', hidden })
const base = (): { graph: TimesGraph; bots: TimesBot[] } => ({
  graph: {
    nodes: [{ id: 'vendas', name: 'Vendas', parentId: null, managerBotId: 'vera', position: 0 }],
    members: [loose('donna', 'manager'), loose('hype'), loose('prosa'), loose('feed1'), loose('feed2'), { ...loose('vera', 'manager'), teamId: 'vendas' }, { ...loose('rank-v'), teamId: 'vendas', reportsTo: { kind: 'bot', botId: 'vera' } }, loose('canal-oculto')],
    routes: [{ fromBotId: 'prosa', toBotId: 'vera', state: 'proposed' }],
  },
  bots: [bot('donna', 'Donna', 'Marketing'), bot('hype', 'Hype', 'Marketing'), bot('prosa', 'Prosa', 'Escritor'), bot('feed1', 'Feed', 'Redes sociais'), bot('feed2', 'Feed Loja', 'redes sociais'),
    bot('vera', 'Vera', 'Vendas'), bot('rank-v', 'Rank', 'SEO & AEO'), bot('canal-oculto', 'Canal', 'Youtuber', true)],
})
const ROLE = Object.fromEntries(ROLES.map((r) => [r.id, r]))

test('a janela lista exatamente quem já existe e quem nasce: só Sem time conta; principal, escondido e outra área não', () => {
  const { graph, bots } = base()
  const plano = planoMarketing(graph, bots, 'donna')
  assert.equal(plano.bloqueio, null)
  const linhas = linhasMarketing(plano)
  assert.deepEqual(linhas.map((l) => [l.role.name, l.estado, l.estado === 'existe' ? l.botId : null]), [
    ['Hype', 'existe', 'hype'], ['Feed', 'existe', 'feed1'], ['Prosa', 'existe', 'prosa'],
    ['Rank', 'criar', null], ['Canal', 'criar', null], ['Traço', 'criar', null], ['Pixel', 'criar', null],
  ])
  assert.deepEqual(plano.vagas.find((v) => v.role.id === 'seo')!.fora.map((f) => [f.bot.id, f.teamId]), [['rank-v', 'vendas']], 'Rank de Vendas aparece como "está em Vendas"')
  assert.deepEqual(plano.vagas.find((v) => v.role.id === 'social')!.candidatos.map((b) => b.id), ['feed1', 'feed2'])
  assert.deepEqual(linhasMarketing(plano, { social: 'feed2' })[1], { role: ROLE.social, chefe: false, estado: 'existe', botId: 'feed2' }, 'o dono escolhe entre dois')
})

test('equipe: Hype chefe e Gerente, 6 membros na área reportando a ele; o resto do quadro fica igual', () => {
  const { graph } = base()
  const membros = ['feed1', 'prosa', 'n-rank', 'n-canal', 'n-traco', 'n-pixel']
  const before = { ...graph, members: [...graph.members, ...membros.filter((m) => m.startsWith('n-')).map((m) => loose(m))] }
  const g = equipeMarketing(before, 'mkt', 'hype', membros)
  assert.equal(localRefusal(g, 'donna'), null)
  assert.deepEqual(g.nodes.find((n) => n.id === 'mkt'), { id: 'mkt', name: 'Marketing', parentId: null, managerBotId: 'hype', position: 1 })
  const hype = g.members.find((m) => m.botId === 'hype')!
  assert.deepEqual(hype, { ...loose('hype', 'manager'), teamId: 'mkt' }, 'o chefe só vira Gerente da área; continua reportando a Você')
  for (const id of membros) {
    assert.deepEqual(g.members.find((m) => m.botId === id), { ...loose(id), teamId: 'mkt', reportsTo: { kind: 'bot', botId: 'hype' } })
  }
  const others = (x: TimesGraph) => x.members.filter((m) => m.botId !== 'hype' && !membros.includes(m.botId))
  assert.deepEqual(others(g), others(before))
  assert.deepEqual(g.routes, before.routes, 'nenhuma rota nova')
  assert.deepEqual(g.nodes.filter((n) => n.id !== 'mkt'), before.nodes)
})

test('Marketing de primeiro nível já existe: o modelo bloqueia e não sobrescreve; rascunho em conferência também bloqueia', () => {
  const { graph, bots } = base()
  graph.nodes.push({ id: 'x', name: ' marketing ', parentId: null, managerBotId: null, position: 1 })
  assert.match(pt(planoMarketing(graph, bots, 'donna').bloqueio), /Já existe uma área Marketing/)
  assert.match(pt(planoMarketing(base().graph, bots, 'donna', true).bloqueio), /Salve ou descarte/)
})

test('bot novo pelo caminho normal: função e rosto do preset, motor padrão, Pedir, sem conector, conta ou skill; regras nas instruções', () => {
  const d = rascunhoMarketing(ROLE.seo!, [{ id: 'codex', available: true } as never])
  assert.equal(d.name, 'Rank')
  assert.equal(d.title, 'SEO & AEO')
  assert.equal(d.backend, 'codex')
  assert.equal(d.model, undefined, 'modelo padrão do motor')
  assert.equal(d.permission, 'ask')
  assert.equal(d.permissionChosenByUser, true)
  assert.deepEqual([d.connectors, d.logins, d.skills, d.computer, d.sharedMemory, d.cwd], [[], [], undefined, undefined, undefined, ''])
  assert.match(d.description, /Regras da Equipe Marketing:\n- Fale com o dono só pela resposta final/)
  assert.ok(d.description.length <= 4000)
})

function fakeDeps(failAt: string | null = null) {
  const log: string[] = []
  let graph = base().graph
  let bots = base().bots
  const deps: Dependencias = {
    motores: async () => [{ id: 'codex', available: true } as never],
    novoId: () => 'mkt',
    async criar(d: BotDraft) {
      log.push(`criar:${d.name}`)
      if (d.name === failAt) throw new Error('motor fora')
      const id = `n-${d.name}`
      graph = { ...graph, members: [...graph.members, loose(id)] }
      bots = [...bots, bot(id, d.name, d.title!)]
      return { id, name: d.name }
    },
    async sincronizar() { log.push('sincronizar'); return { ...graph, bots, primaryBotId: 'donna' } },
    async propor(label, change) { log.push(`propor:${pt(label)}`); graph = change(graph); log.push('aplicado'); return true },
  }
  return { deps, log, graph: () => graph, bots: () => bots, setGraph: (g: TimesGraph) => { graph = g } }
}

test('nada nasce antes de confirmar: planejar e listar não chamam nada; confirmar cria só os "Criar agora" e propõe a equipe', async () => {
  const { graph, bots } = base()
  const f = fakeDeps()
  const linhas = linhasMarketing(planoMarketing(graph, bots, 'donna'))
  assert.deepEqual(f.log, [], 'abrir a janela (e Cancelar, que só a fecha) não cria nem grava nada')
  assert.deepEqual(await executarMarketing(linhas, f.deps), { ok: true })
  assert.deepEqual(f.log, ['criar:Rank', 'criar:Canal', 'criar:Traço', 'criar:Pixel', 'sincronizar', 'propor:Equipe Marketing', 'aplicado'])
  const g = f.graph()
  assert.equal(g.nodes.find((n) => n.id === 'mkt')!.managerBotId, 'hype')
  assert.deepEqual(g.members.filter((m) => m.teamId === 'mkt' && m.botId !== 'hype').map((m) => m.reportsTo), Array(6).fill({ kind: 'bot', botId: 'hype' }))
})

test('falha no meio: para ali, nada vai ao quadro e a janela diz quem já nasceu; de novo, eles são reaproveitados', async () => {
  const f = fakeDeps('Traço')
  const { bots } = base()
  const r = await executarMarketing(linhasMarketing(planoMarketing(f.graph(), bots, 'donna')), f.deps)
  assert.deepEqual({ ...r, erro: pt((r as { erro: Frase }).erro) }, { ok: false, criados: ['Rank', 'Canal'], erro: 'Não foi possível criar Traço: motor fora' })
  assert.ok(!f.log.some((l) => l.startsWith('propor')), 'nada vai ao quadro')
  assert.equal(f.graph().nodes.length, 1)
  // Os criados têm a função do preset: na próxima leitura entram como "Já existe".
  const depois = [...bots, bot('n-Rank', 'Rank', 'SEO & AEO'), bot('n-Canal', 'Canal', 'Youtuber')]
  const linhas = linhasMarketing(planoMarketing(f.graph(), depois, 'donna'))
  assert.deepEqual(linhas.filter((l) => l.estado === 'criar').map((l) => l.role.name), ['Traço', 'Pixel'])
  assert.equal(MARKETING.membros.length, 6)
})

test('R1: uma execução por sessão; confirmar de novo (janela reaberta) durante a criação não cria outra vez', async () => {
  const f = fakeDeps()
  let solta!: () => void
  const presa = new Promise<void>((r) => { solta = r })
  const criar = f.deps.criar
  f.deps.criar = async (d) => { await presa; return criar(d) }
  const exec = new ExecucaoMarketing()
  const linhas = linhasMarketing(planoMarketing(f.graph(), f.bots(), 'donna'))
  const primeira = exec.executar(linhas, f.deps)
  assert.equal(exec.rodando, true)
  const emCurso = await exec.executar(linhas, f.deps)
  assert.deepEqual({ ...emCurso, erro: pt((emCurso as { erro: Frase }).erro) }, { ok: false, criados: [], erro: 'O modelo Marketing já está em andamento.' })
  solta()
  assert.deepEqual(await primeira, { ok: true })
  assert.equal(f.log.filter((l) => l === 'criar:Rank').length, 1)
})

test('R2: a releitura falha depois de criar; Tentar de novo reaproveita os criados e nunca os recria', async () => {
  const f = fakeDeps()
  const sinc = f.deps.sincronizar
  let falhar = true
  f.deps.sincronizar = async () => { if (falhar) throw new Error('sem link'); return sinc() }
  const exec = new ExecucaoMarketing()
  // O quadro da janela ainda é o de antes (sem os novos): as linhas continuam "Criar agora".
  const antes = { graph: f.graph(), bots: f.bots() }
  const linhas = linhasMarketing(planoMarketing(antes.graph, antes.bots, 'donna'))
  const semLeitura = await exec.executar(linhas, f.deps)
  assert.deepEqual({ ...semLeitura, erro: pt((semLeitura as { erro: Frase }).erro) }, { ok: false, criados: ['Rank', 'Canal', 'Traço', 'Pixel'], erro: 'Não foi possível reler o quadro.' })
  assert.deepEqual(exec.escolhas, { seo: 'n-Rank', youtuber: 'n-Canal', designer: 'n-Traço', metaads: 'n-Pixel' })
  falhar = false
  assert.deepEqual(await exec.executar(linhasMarketing(planoMarketing(antes.graph, antes.bots, 'donna'), exec.escolhas), f.deps), { ok: true })
  assert.equal(f.log.filter((l) => l.startsWith('criar:')).length, 4, 'nenhum bot criado duas vezes')
  // No quadro novo, os criados entram como "Já existe".
  const depois = linhasMarketing(planoMarketing({ ...base().graph, members: f.graph().members.map((m) => ({ ...m, teamId: null, parentBotId: null })) }, f.bots(), 'donna'), { seo: 'n-Rank' })
  assert.deepEqual(depois[3], { role: ROLE.seo, chefe: false, estado: 'existe', botId: 'n-Rank' })
})

test('R3: escolhido saiu de Sem time durante a criação (ou sem nada a criar): relê, para sem tocar no quadro e diz quem', async () => {
  for (const comNovos of [true, false]) {
    const f = fakeDeps()
    const linhas = linhasMarketing(planoMarketing(f.graph(), f.bots(), 'donna'))
    const todas = comNovos ? linhas : linhas.map((l, i) => (l.estado === 'criar' ? { role: l.role, chefe: l.chefe, estado: 'existe' as const, botId: ['n-x', 'n-y', 'n-z', 'n-w'][i - 3]! } : l))
    if (!comNovos) f.setGraph({ ...f.graph(), members: [...f.graph().members, ...['n-x', 'n-y', 'n-z', 'n-w'].map((id) => loose(id))] })
    // Feed vai para Vendas em outro aparelho antes da releitura.
    f.setGraph({ ...f.graph(), members: f.graph().members.map((m) => (m.botId === 'feed1' ? { ...m, teamId: 'vendas' } : m)) })
    const r = await executarMarketing(todas, f.deps)
    assert.equal(r.ok, false)
    assert.match(pt((r as { erro: Frase }).erro), /O quadro mudou: Feed/)
    assert.ok(f.log.includes('sincronizar'), 'relê mesmo sem bot a criar')
    assert.ok(!f.log.includes('aplicado'), 'nada vai ao quadro')
  }
})

test('R3 (rodada 2): o rascunho muda durante a releitura (Feed para Vendas); a conferência é na base aplicada e recusa', async () => {
  const f = fakeDeps()
  const sinc = f.deps.sincronizar
  // O Hub relido ainda tem o Feed em Sem time; o rascunho do quadro, onde a mudança é aplicada, já não.
  f.deps.sincronizar = async () => { const lido = await sinc(); f.setGraph({ ...f.graph(), members: f.graph().members.map((m) => (m.botId === 'feed1' ? { ...m, teamId: 'vendas' } : m)) }); return lido }
  const r = await executarMarketing(linhasMarketing(planoMarketing(f.graph(), f.bots(), 'donna')), f.deps)
  assert.equal(r.ok, false)
  assert.match(pt((r as { erro: Frase }).erro), /O quadro mudou: Feed não está mais em Sem equipe/)
  assert.ok(!f.log.includes('aplicado'))
  assert.equal(f.graph().nodes.length, 1, 'nenhuma área nova no rascunho')
  assert.equal(f.graph().members.find((m) => m.botId === 'feed1')!.teamId, 'vendas', 'o Feed fica onde o dono pôs')
})

test('R4: bot com subbots não é "Já existe" (iria junto, sem aparecer); aparece como "tem subbots" e o modelo cria outro', () => {
  const { graph, bots } = base()
  graph.members.push({ ...loose('ajudante'), parentBotId: 'hype' })
  const plano = planoMarketing(graph, [...bots, bot('ajudante', 'Ajudante', '')], 'donna')
  const hype = plano.vagas[0]!
  assert.deepEqual(hype.candidatos, [])
  assert.deepEqual(hype.fora.map((f) => [f.bot.id, f.subbots]), [['hype', true]])
  assert.equal(linhasMarketing(plano)[0]!.estado, 'criar')
})
