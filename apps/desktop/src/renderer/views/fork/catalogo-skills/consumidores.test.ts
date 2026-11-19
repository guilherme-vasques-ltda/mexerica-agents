import assert from 'node:assert/strict'
import test from 'node:test'
import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { InstalledSkillV2 } from '../../../store/fork-skills.ts'
import { estadoDaFolha, lerInstalada, mesmaOrigem, origemDaFonte, ProjecaoSkills, type FonteSkills, type LeituraFolha } from './consumidores.ts'
import { textoDe, cru as cruDe } from './textos.ts'

// D70: só as fronteiras dos consumidores fora da página — a projeção do Plugins e a folha dos
// ajustes do bot. Hub falso sintético declarado pelo DTO R7; nenhum Hub, conta ou rede reais.

const NS = '33333333-3333-4333-8333-333333333333'
const skill = (id: string, seq: number): InstalledSkillV2 => ({
  id, name: id, description: 'Synthetic.', source: 'custom', path: `/synthetic/${id}/SKILL.md`, receipt: null, installationId: `${NS}:${seq}`, contentStatus: 'verified',
})

/** Um cliente cujas respostas o teste solta na ordem que quiser. */
function cliente(nome: string) {
  const chamadas: [string, unknown][] = []
  const presos: { soltar: (v: unknown) => void; falhar: (e: Error) => void }[] = []
  const client: SkillClient = {
    async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      chamadas.push([metodo, corpo])
      return await new Promise<T>((soltar, falhar) => presos.push({ soltar: soltar as (v: unknown) => void, falhar }))
    },
  }
  return { nome, client, chamadas, presos }
}
const esperar = () => new Promise((r) => setTimeout(r, 0))

function fonte(hub: string, client: SkillClient, geracao = 1) {
  const f = {
    hello: { hostId: hub } as { hostId: string } | null,
    connectedClient: client as SkillClient | null,
    principalEpoch: geracao,
    entregar: client,
    ready: async () => f.entregar,
  }
  return f satisfies FonteSkills
}

test('projeção do Plugins: leitura forte; resposta de outro Hub, conexão ou geração não entra; outro Hub não mostra nada', async () => {
  const a = cliente('a')
  const f = fonte('hub-a', a.client)
  const projecao = new ProjecaoSkills()
  const lendo = projecao.ler(f)
  await esperar()
  assert.deepEqual(a.chamadas, [['skills', { skillProtocol: 2 }]])
  a.presos.shift()!.soltar({ skillProtocol: 2, items: [skill('alpha', 1)] })
  assert.equal(await lendo, true)
  assert.deepEqual(projecao.itens(f).map((s) => s.id), ['alpha'])

  // Troca de Hub: nada do A aparece no B, nem antes de reler.
  f.hello = { hostId: 'hub-b' }
  assert.deepEqual(projecao.itens(f), [])
  f.hello = { hostId: 'hub-a' }

  // Leitura que chega depois de trocar de Hub, de link ou de geração não entra.
  for (const mudar of [
    () => { f.hello = { hostId: 'hub-b' } },
    () => { f.connectedClient = cliente('outro').client },
    () => { f.principalEpoch += 1 },
  ]) {
    const antes = { hello: f.hello, client: f.connectedClient, geracao: f.principalEpoch }
    const tardia = projecao.ler(f)
    await esperar()
    mudar()
    a.presos.shift()!.soltar({ skillProtocol: 2, items: [skill('beta', 2)] })
    assert.equal(await tardia, false)
    Object.assign(f, { hello: antes.hello, connectedClient: antes.client, principalEpoch: antes.geracao })
    assert.deepEqual(projecao.itens(f).map((s) => s.id), ['alpha'])
  }

  // Duas leituras: a mais velha que chega depois não sobrescreve a mais nova.
  const velha = projecao.ler(f)
  const nova = projecao.ler(f)
  await esperar()
  a.presos[1]!.soltar({ skillProtocol: 2, items: [skill('nova', 3)] })
  assert.equal(await nova, true)
  a.presos[0]!.soltar({ skillProtocol: 2, items: [skill('velha', 4)] })
  assert.equal(await velha, false)
  a.presos.splice(0)
  assert.deepEqual(projecao.itens(f).map((s) => s.id), ['nova'])
})

test('projeção do Plugins: Hub sem o protocolo forte (inclusive vazio) não projeta nada; erro passageiro conserva a última do mesmo Hub', async () => {
  const a = cliente('a')
  const f = fonte('hub-a', a.client)
  const projecao = new ProjecaoSkills()
  const primeira = projecao.ler(f)
  await esperar()
  a.presos.shift()!.soltar({ skillProtocol: 2, items: [skill('alpha', 1)] })
  await primeira
  const erro = projecao.ler(f)
  await esperar()
  a.presos.shift()!.falhar(new Error('Hub ocupado'))
  assert.equal(await erro, false)
  assert.deepEqual(projecao.itens(f).map((s) => s.id), ['alpha'], 'erro passageiro não apaga')
  for (const antigo of [{ items: [skill('alpha', 1)] }, { items: [] }]) {
    const lendo = projecao.ler(f)
    await esperar()
    a.presos.shift()!.soltar(antigo)
    assert.equal(await lendo, true)
    assert.deepEqual(projecao.itens(f), [], 'sem prova de versão, nenhuma skill')
  }
  // Sem link vivo, nem tenta.
  f.connectedClient = null
  assert.equal(await projecao.ler(f), false)
  assert.equal(a.chamadas.length, 4)
})

test('folha dos ajustes do bot: leitura forte presa ao Hub em que abriu', async () => {
  const a = cliente('a')
  const f = fonte('hub-a', a.client)
  const origem = origemDaFonte(f)
  const detalhes = { skillProtocol: 2, markdown: 'TEXT A', catalog: null, receipt: null, installation: skill('alpha', 1) }

  const lendo = lerInstalada(f, origem, 'alpha')
  await esperar()
  assert.deepEqual(a.chamadas, [['skillDetails', { id: 'alpha', skillProtocol: 2 }]])
  a.presos.shift()!.soltar(detalhes)
  assert.equal((await lendo)?.markdown, 'TEXT A')

  // A resposta chega depois de trocar de Hub: nada aparece.
  const tardia = lerInstalada(f, origem, 'alpha')
  await esperar()
  f.hello = { hostId: 'hub-b' }
  a.presos.shift()!.soltar(detalhes)
  assert.equal(await tardia, null)

  // ready() entrega o link de outro Hub: nenhum pedido por ele.
  f.hello = { hostId: 'hub-a' }
  const b = cliente('b')
  f.entregar = b.client
  assert.equal(await lerInstalada(f, origem, 'alpha'), null)
  assert.deepEqual(b.chamadas, [])

  // Sem Hub na abertura, ou outro Hub agora: nem pergunta.
  assert.equal(await lerInstalada(f, null, 'alpha'), null)
  f.hello = { hostId: 'hub-c' }
  assert.equal(await lerInstalada(f, origem, 'alpha'), null)
  assert.equal(a.chamadas.length, 2)
})

// R10 (revisão Sol 9, R9-F1): a folha retida fica presa à tupla Hub + link + geração e só mostra uma
// leitura válida agora; nunca a linha que abriu a folha nem uma leitura anterior.

test('folha: reconexão ao mesmo Hub e A→B→A sem render não publicam a leitura pendente', async () => {
  const a = cliente('a')
  const f = fonte('hub-a', a.client)
  const origem = origemDaFonte(f)
  assert.deepEqual(origem, { hub: 'hub-a', client: a.client, geracao: 1 })
  const detalhes = { skillProtocol: 2, markdown: 'TEXT A', catalog: null, receipt: null, installation: skill('alpha', 1) }

  // Nova geração (hello do mesmo Hub) com a resposta ainda pendente.
  const pendente = lerInstalada(f, origem, 'alpha')
  await esperar()
  f.principalEpoch += 1
  a.presos.shift()!.soltar(detalhes)
  assert.equal(await pendente, null)
  assert.equal(mesmaOrigem(f, origem), false)
  assert.equal(await lerInstalada(f, origem, 'alpha'), null, 'origem velha nem pergunta')
  assert.equal(a.chamadas.length, 1)

  // A→B→A sem render: mesmo hostId, outro link e outra geração.
  const f2 = fonte('hub-a', a.client)
  const origem2 = origemDaFonte(f2)
  const outra = lerInstalada(f2, origem2, 'alpha')
  await esperar()
  const c = cliente('c')
  f2.hello = { hostId: 'hub-b' }; f2.connectedClient = cliente('b').client; f2.principalEpoch += 1
  f2.hello = { hostId: 'hub-a' }; f2.connectedClient = c.client; f2.principalEpoch += 1
  a.presos.shift()!.soltar(detalhes)
  assert.equal(await outra, null)
  assert.deepEqual(c.chamadas, [])

  // Gesto novo na conexão de agora lê normalmente.
  f2.entregar = c.client
  const nova = lerInstalada(f2, origemDaFonte(f2), 'alpha')
  await esperar()
  c.presos.shift()!.soltar(detalhes)
  assert.equal((await nova)?.markdown, 'TEXT A')
})

test('estado da folha: só a leitura válida desta origem mostra a skill; o resto é neutro', () => {
  const a = cliente('a').client
  const f = fonte('hub-a', a)
  const origem = origemDaFonte(f)!
  const detalhes = { skillProtocol: 2 as const, markdown: 'TEXT A', catalog: null, receipt: null, installation: skill('alpha', 1) }
  const pronta: LeituraFolha = { origem, tentativa: 0, resultado: { detalhes } }
  const erro = (codigo: string | null, msg = 'falhou'): LeituraFolha => ({ origem, tentativa: 0, resultado: { erro: cruDe(msg), codigo } })

  assert.deepEqual(estadoDaFolha(f, origem, 0, pronta, false), { tipo: 'pronta', detalhes })
  assert.deepEqual(estadoDaFolha(f, origem, 0, null, false), { tipo: 'lendo' })
  assert.deepEqual(estadoDaFolha(f, origem, 1, pronta, false), { tipo: 'lendo' }, 'leitura de outra tentativa não aparece')
  assert.deepEqual(estadoDaFolha(f, origem, 0, { ...pronta, origem: { ...origem } }, false), { tipo: 'lendo' }, 'leitura de outra origem não aparece')
  assert.deepEqual(estadoDaFolha(f, origem, 0, pronta, true), { tipo: 'semProtocolo' }, 'protocolo desmentido pela página esconde a leitura')
  assert.deepEqual(estadoDaFolha(f, origem, 0, erro('skill_installation_unsupported'), false), { tipo: 'semProtocolo' })
  assert.deepEqual(estadoDaFolha(f, origem, 0, erro('skill_installation_changed'), false), { tipo: 'conteudo', status: 'changed' })
  assert.deepEqual(estadoDaFolha(f, origem, 0, erro('skill_installation_unavailable'), false), { tipo: 'conteudo', status: 'unavailable' })
  const ocupado = estadoDaFolha(f, origem, 0, erro(null, 'Hub ocupado'), false)
  assert.equal(ocupado.tipo, 'erro')
  assert.equal(ocupado.tipo === 'erro' ? textoDe(ocupado.mensagem, 'pt-BR') : null, 'Hub ocupado')
  assert.deepEqual(estadoDaFolha(f, null, 0, null, false), { tipo: 'conexaoMudou', semLink: true })

  // Troca de contexto com a leitura pronta: nada dela aparece.
  const b = fonte('hub-b', cliente('b').client)
  assert.deepEqual(estadoDaFolha(b, origem, 0, pronta, false), { tipo: 'outroHub' })
  const reconectada = fonte('hub-a', a, 2)
  assert.deepEqual(estadoDaFolha(reconectada, origem, 0, pronta, false), { tipo: 'conexaoMudou', semLink: false })
  const semLink = fonte('hub-a', a)
  semLink.connectedClient = null
  assert.deepEqual(estadoDaFolha(semLink, origem, 0, pronta, false), { tipo: 'conexaoMudou', semLink: true })
  const outroLink = fonte('hub-a', cliente('a2').client)
  assert.deepEqual(estadoDaFolha(outroLink, origem, 0, pronta, false), { tipo: 'conexaoMudou', semLink: false })
  for (const fora of [b, reconectada, semLink, outroLink]) {
    const e = estadoDaFolha(fora, origem, 0, pronta, false)
    assert.equal('detalhes' in e, false, 'só o estado pronto carrega dados da skill')
  }
})
