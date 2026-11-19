import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'
import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { SkillReviewRequest } from '../../../../shared/fork-skill-review.ts'
import { CatalogoStore, type IntencaoHub } from './catalogo-store.ts'
import { ErroRevisao, canonico, codigoDeRevisao, destinoDoRetido, diffDeLinhas, pedidoDeRevisao, pedirRevisao } from './revisao.ts'
import { textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// Dados sintéticos; digests calculados como o Hub (SHA-256 hex dos bytes e do envelope canônico).
const sha = (s: string) => createHash('sha256').update(s, 'utf8').digest('hex')
const INST = '11111111-1111-4111-8111-111111111111:7'
const BASE = '---\nname: resumir-reuniao\ndescription: Resume reuniões.\n---\n\n1. Leia tudo.\n2. Liste as decisões.\n'
const NOVO = BASE.replace('2. Liste as decisões.', '2. Liste as decisões em frases curtas.\n3. Não invente datas.')
const BOTS = [{ id: 'donna', name: 'Donna', selectionVersion: 'a'.repeat(32) }, { id: 'bot-x', name: 'Bot X', selectionVersion: 'b'.repeat(32) }]

/** A resposta que o Hub d2a monta para o pedido; `mexer` adultera o envelope depois de o digest ser calculado. */
function resposta(pedido: SkillReviewRequest, mexer?: (r: Record<string, unknown>) => void) {
  const review: Record<string, unknown> = {
    reviewProtocol: 1, effect: 'none', id: 'resumir-reuniao', installationId: pedido.expectedInstallationId,
    base: { revision: 1, bodyDigest: sha(BASE), markdown: BASE },
    proposal: pedido.proposal ? { revision: 2, bodyDigest: sha(pedido.proposal.markdown), markdown: pedido.proposal.markdown } : null,
    affectedBots: BOTS,
  }
  review.comparisonDigest = sha(canonico(review))
  mexer?.(review)
  return { skillProtocol: 2, review }
}

function cliente(responder: (metodo: string, corpo: Record<string, unknown>) => unknown) {
  const chamadas: [string, unknown][] = []
  const c: SkillClient = {
    call: async <T,>(metodo: string, corpo?: unknown) => {
      chamadas.push([metodo, structuredClone(corpo)])
      const r = responder(metodo, corpo as Record<string, unknown>)
      if (r instanceof Error) throw r
      return r as T
    },
  }
  return { c, chamadas }
}
const hubBom = () => cliente((_m, corpo) => resposta(corpo.review as SkillReviewRequest))
const comparar = () => pedidoDeRevisao(INST, { expectedBodyDigest: sha(BASE), markdown: NOVO })
const recusa = async (p: Promise<unknown>, codigo: string) => {
  await assert.rejects(p, (e: unknown) => codigoDeRevisao(e) === codigo)
}

test('abrir e comparar mandam só skillDetails estrito, sem proposal nula, e conferem a resposta', async () => {
  const { c, chamadas } = hubBom()
  const base = await pedirRevisao(c, 'resumir-reuniao', pedidoDeRevisao(INST))
  assert.equal(base.proposal, null)
  assert.equal(base.base.markdown, BASE)
  const r = await pedirRevisao(c, 'resumir-reuniao', comparar())
  assert.equal(r.proposal?.markdown, NOVO)
  assert.equal(r.proposal?.revision, 2)
  assert.deepEqual(r.affectedBots, BOTS)
  assert.deepEqual(chamadas, [
    ['skillDetails', { skillProtocol: 2, id: 'resumir-reuniao', review: { protocol: 1, expectedInstallationId: INST } }],
    ['skillDetails', { skillProtocol: 2, id: 'resumir-reuniao', review: { protocol: 1, expectedInstallationId: INST, proposal: { expectedBodyDigest: sha(BASE), markdown: NOVO } } }],
  ])
  // Nenhuma mutação: nem installSkill, nem removeSkill, nem seleção.
  assert.ok(chamadas.every(([m]) => m === 'skillDetails'))
})

test('resposta adulterada, de outro alvo ou com alcance trocado nunca vira comparação', async () => {
  const casos: [string, (r: Record<string, unknown>) => void][] = [
    ['digest do envelope errado', (r) => { r.comparisonDigest = 'f'.repeat(64) }],
    ['bots trocados depois do digest', (r) => { r.affectedBots = [BOTS[0]] }],
    ['bots vazios depois do digest', (r) => { r.affectedBots = [] }],
    ['versão de seleção fora do formato', (r) => { const { comparisonDigest: _, ...resto } = r; r.affectedBots = [{ ...BOTS[0], selectionVersion: 'x' }]; r.comparisonDigest = sha(canonico({ ...resto, affectedBots: r.affectedBots })) }],
    ['efeito diferente de none', (r) => { r.effect = 'applied' }],
    ['outra encarnação', (r) => { r.installationId = '11111111-1111-4111-8111-111111111111:8' }],
    ['outra skill', (r) => { r.id = 'outra' }],
    ['campo a mais', (r) => { r.approval = true }],
    ['corpo da base não confere', (r) => { (r.base as Record<string, unknown>).markdown = `${BASE}x` }],
    ['proposta com outros bytes', (r) => { (r.proposal as Record<string, unknown>).markdown = BASE }],
    ['proposta ausente', (r) => { r.proposal = null }],
    ['revisão candidata fora da sequência', (r) => { (r.proposal as Record<string, unknown>).revision = 5 }],
  ]
  for (const [nome, mexer] of casos) {
    const { c } = cliente((_m, corpo) => resposta(corpo.review as SkillReviewRequest, mexer))
    await recusa(pedirRevisao(c, 'resumir-reuniao', comparar()), 'resposta_invalida').catch((e) => { throw new Error(nome, { cause: e }) })
  }
  const semEnvelope = cliente(() => ({ review: resposta(pedidoDeRevisao(INST)).review }))
  await recusa(pedirRevisao(semEnvelope.c, 'resumir-reuniao', pedidoDeRevisao(INST)), 'resposta_invalida')
})

test('recusas do Hub mantêm o código; Hub sem review vira hub_sem_revisao; transporte mantém a mensagem', async () => {
  for (const codigo of ['skill_review_base_unknown', 'skill_review_base_changed', 'skill_review_scope_unavailable', 'skill_review_manual_only', 'skill_review_invalid', 'skill_installation_stale']) {
    const { c } = cliente(() => new Error(codigo))
    await recusa(pedirRevisao(c, 'resumir-reuniao', comparar()), codigo)
  }
  await recusa(pedirRevisao(cliente(() => new Error('Escolha uma habilidade por id ou source')).c, 'resumir-reuniao', pedidoDeRevisao(INST)), 'hub_sem_revisao')
  await recusa(pedirRevisao(cliente(() => ({ skillProtocol: 2, markdown: BASE, catalog: null, receipt: null, installation: null })).c, 'resumir-reuniao', pedidoDeRevisao(INST)), 'hub_sem_revisao')
  await assert.rejects(pedirRevisao(cliente(() => new Error('timeout')).c, 'resumir-reuniao', pedidoDeRevisao(INST)), (e: unknown) => codigoDeRevisao(e) === null && (e as Error).message === 'timeout')
  // Acima de 32 KiB nem sai pedido.
  assert.throws(() => pedidoDeRevisao(INST, { expectedBodyDigest: sha(BASE), markdown: 'á'.repeat(16 * 1024 + 1) }), ErroRevisao)
})

const reconstruir = (linhas: ReturnType<typeof diffDeLinhas>) => ({
  antes: linhas.filter((l) => l.tipo !== 'adicionada').map((l) => l.texto).join('\n'),
  depois: linhas.filter((l) => l.tipo !== 'removida').map((l) => l.texto).join('\n'),
})

test('diff integral: toda linha dos dois documentos, na ordem, remoção antes da adição', () => {
  const linhas = diffDeLinhas(BASE, NOVO)
  assert.deepEqual(reconstruir(linhas), { antes: BASE, depois: NOVO })
  const trocadas = linhas.filter((l) => l.tipo !== 'igual').map((l) => l.tipo)
  assert.deepEqual(trocadas, ['removida', 'adicionada', 'adicionada'])
  assert.deepEqual(diffDeLinhas(BASE, BASE).filter((l) => l.tipo !== 'igual'), [])
})

test('diff grande sai do LCS limitado sem cortar nada', () => {
  const a = Array.from({ length: 3000 }, (_, i) => `a${i}`).join('\n')
  const b = Array.from({ length: 3000 }, (_, i) => `b${i}`).join('\n')
  const inicio = performance.now()
  const linhas = diffDeLinhas(`topo\n${a}\nfim`, `topo\n${b}\nfim`)
  assert.ok(performance.now() - inicio < 500)
  assert.equal(linhas.length, 6002)
  assert.deepEqual(reconstruir(linhas), { antes: `topo\n${a}\nfim`, depois: `topo\n${b}\nfim` })
})

test('a loja só revisa na conexão exata do gesto: A→B→A, reconexão e cliente trocado não entregam', async () => {
  let vivo = { hub: 'A', client: null as SkillClient | null, geracao: 1 }
  const { c, chamadas } = hubBom()
  vivo.client = c
  let entregue: SkillClient = c
  const loja = new CatalogoStore(async () => entregue, () => {}, () => vivo)
  loja.sincronizar('A', { adiarCarga: true })
  const origem = loja.capturarIntencao() as IntencaoHub
  assert.ok((await loja.revisar('resumir-reuniao', pedidoDeRevisao(INST), origem))?.base)

  // Reconexão ao mesmo Hub (nova geração): a origem antiga não pede nem entrega.
  vivo = { ...vivo, geracao: 2 }
  const antes = chamadas.length
  assert.equal(await loja.revisar('resumir-reuniao', pedidoDeRevisao(INST), origem), null)
  assert.equal(chamadas.length, antes)

  // A→B→A: mesmo hostId, outra época; a origem de antes não vale.
  const nova = loja.capturarIntencao() as IntencaoHub
  loja.sincronizar('B', { adiarCarga: true })
  loja.sincronizar('A', { adiarCarga: true })
  assert.equal(await loja.revisar('resumir-reuniao', pedidoDeRevisao(INST), nova), null)
  assert.equal(chamadas.length, antes)

  // ready() entregou outro cliente: recusa antes do RPC.
  const atual = loja.capturarIntencao() as IntencaoHub
  entregue = cliente(() => { throw new Error('não deveria chamar') }).c
  await assert.rejects(loja.revisar('resumir-reuniao', pedidoDeRevisao(INST), atual), (e: unknown) => /conexão com o Hub mudou/.test(textoDe((e as { frase: Frase }).frase, 'pt-BR')))

  // Resposta que chega depois da troca é descartada.
  let soltar: () => void = () => {}
  entregue = { call: <T,>(_m: string, corpo?: unknown) => new Promise<T>((ok) => { soltar = () => ok(resposta((corpo as { review: SkillReviewRequest }).review) as T) }) }
  vivo = { ...vivo, client: entregue }
  const preso = loja.capturarIntencao() as IntencaoHub
  const pendente = loja.revisar('resumir-reuniao', pedidoDeRevisao(INST), preso)
  await new Promise((r) => setTimeout(r, 0))
  vivo = { ...vivo, geracao: 9 }
  soltar()
  assert.equal(await pendente, null)
})

test('D2A-UI1: texto retido só volta no mesmo Hub e na mesma encarnação; nunca em outro Hub ou instalação', () => {
  const retido = { hub: 'A', id: 'resumir-reuniao', installationId: INST }
  const mesma = { id: 'resumir-reuniao', installationId: INST }
  assert.equal(destinoDoRetido(retido, mesma, 'A'), 'adota')
  // Outro Hub com a mesma ID e a mesma encarnação (colisão): não adota.
  assert.equal(destinoDoRetido(retido, mesma, 'B'), 'outroHub')
  assert.equal(destinoDoRetido(retido, mesma, null), 'outroHub')
  assert.equal(destinoDoRetido(retido, { id: 'resumir-reuniao', installationId: '11111111-1111-4111-8111-111111111111:8' }, 'A'), 'outraInstalacao')
  // Outra skill ou nada retido: a folha segue normal.
  assert.equal(destinoDoRetido(retido, { id: 'outra', installationId: INST }, 'A'), null)
  assert.equal(destinoDoRetido(null, mesma, 'A'), null)
})
