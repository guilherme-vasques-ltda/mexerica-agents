import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CampoRegras, LIMITE } from './regras.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import { receiptAnchors } from '../recibo/presentation.ts'
import type { Entry } from '../../../../shared/models.ts'

// Hub falso com a mesma regra de versão (CAS) e recibo por clientRequestId do Hub real.
function hub() {
  const state = { version: 0, general: '', teams: {} as Record<string, string> }
  const receipts = new Map<string, number>()
  const calls: { method: string; body: Record<string, unknown> }[] = []
  let drop = false
  const snapshot = () => ({ spaceId: 'p', version: state.version, general: state.general, teams: { ...state.teams } })
  const link = {
    async call(method: string, body: Record<string, unknown> = {}) {
      calls.push({ method, body })
      if (method === 'donnaDelegationRules') return snapshot()
      const id = body.clientRequestId as string
      if (!receipts.has(id)) {
        if (body.expectedVersion !== state.version) throw new Error('delegation_conflict: as regras mudaram; releia antes de salvar.')
        const team = body.teamId as string | null
        if (team) state.teams[team] = body.text as string
        else state.general = body.text as string
        state.version++
        receipts.set(id, state.version)
      }
      if (drop) { drop = false; throw Object.assign(new Error('timeout'), { status: 0 }) }
      return { snapshot: snapshot(), receipt: { clientRequestId: id, version: receipts.get(id) } }
    },
  }
  return { state, calls, link, dropNext: () => { drop = true } }
}

let n = 0
const id = () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}`

test('a página grava a regra geral pela RPC do dono, com versão esperada e chave', async () => {
  const h = hub()
  const campo = new CampoRegras(h.link, null, id)
  await campo.ler()
  assert.equal(campo.estado, 'delegacao.estado.vazio')
  campo.editar('  Coordenadores: Opus 5.5 high.\r\n')
  assert.equal(campo.estado, 'delegacao.estado.naoSalvo')
  await campo.salvar()
  const save = h.calls.find((c) => c.method === 'donnaDelegationRulesSave')!
  assert.deepEqual({ ...save.body, clientRequestId: 'x' }, { teamId: null, text: 'Coordenadores: Opus 5.5 high.', expectedVersion: 0, clientRequestId: 'x' })
  assert.equal(h.state.general, 'Coordenadores: Opus 5.5 high.')
  assert.equal(campo.estado, 'delegacao.estado.salvo')
  assert.equal(campo.versao, 1)
})

test('o campo da equipe grava só a área dele, pela mesma RPC', async () => {
  const h = hub()
  const campo = new CampoRegras(h.link, 'marketing', id)
  await campo.ler()
  campo.editar('Para vídeo, peça ao Youtube.')
  await campo.salvar()
  assert.deepEqual(h.state.teams, { marketing: 'Para vídeo, peça ao Youtube.' })
  assert.equal(h.state.general, '')
  assert.equal(h.calls.at(-1)!.body.teamId, 'marketing')
})

test('conflito de versão: Tentar de novo relê e grava o texto da pessoa', async () => {
  const h = hub()
  const campo = new CampoRegras(h.link, null, id)
  await campo.ler()
  h.state.version = 5
  h.state.general = 'de outro lugar'
  campo.editar('meu texto')
  await campo.salvar()
  assert.equal(campo.fase, 'conflito')
  assert.equal(campo.estado, 'delegacao.estado.conflito')
  assert.equal(campo.texto, 'meu texto')
  await campo.tentarDeNovo()
  assert.equal(h.state.general, 'meu texto')
  assert.equal(campo.versao, 6)
})

test('envio sem resposta: Tentar de novo reenvia a mesma chave e o Hub não grava duas vezes', async () => {
  const h = hub()
  const campo = new CampoRegras(h.link, null, id)
  await campo.ler()
  campo.editar('uma vez')
  h.dropNext()
  await campo.salvar()
  assert.equal(campo.fase, 'falha')
  await campo.tentarDeNovo()
  const saves = h.calls.filter((c) => c.method === 'donnaDelegationRulesSave')
  assert.equal(saves.length, 2)
  assert.equal(saves[0]!.body.clientRequestId, saves[1]!.body.clientRequestId)
  assert.equal(h.state.version, 1)
  assert.equal(campo.estado, 'delegacao.estado.salvo')
})

test('acima do limite não envia', async () => {
  const h = hub()
  const campo = new CampoRegras(h.link, null, id)
  await campo.ler()
  campo.editar('a'.repeat(LIMITE + 1))
  await campo.salvar()
  assert.equal(campo.estado, 'delegacao.falha.limite')
  assert.equal(h.calls.some((c) => c.method === 'donnaDelegationRulesSave'), false)
})

test('recibo do turno: a versão vem do aviso do remetente no mesmo turno', () => {
  const e = (id: string, kind: string, turn: number, data: Entry['data'] = {}): Entry => ({ id, seq: 0, botId: 'byte', rev: 0, kind, turn, data, createdAt: 0, updatedAt: 0 })
  const anchors = receiptAnchors([
    e('u1', 'user', 1),
    e('n1', 'notice', 1, { sourceBotId: 'byte', delegationRulesVersion: 3 }),
    e('n2', 'notice', 1, { sourceBotId: 'outro', delegationRulesVersion: 9 }),
    e('a1', 'agent', 1, { final: true }),
    e('u2', 'user', 2),
    e('a2', 'agent', 2, { final: true }),
  ])
  assert.equal(anchors.get('a1')!.rulesVersion, 3)
  assert.equal(anchors.get('a2')!.rulesVersion, undefined)
  assert.equal(traduzir('pt-BR', 'delegacao.recibo', { versao: 3 }), 'Regras de delegação v3')
  assert.equal(traduzir('en', 'delegacao.recibo', { versao: 3 }), 'Delegation rules v3')
})
