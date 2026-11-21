import assert from 'node:assert/strict'
import test from 'node:test'
import type { LadoALadoEscopo, LadoALadoEscopoEstado, LadoALadoPainel, LadoALadoRef } from '../../../../shared/fork-lado-a-lado.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import {
  acaoSelecaoBarraLateral,
  anuncioMudancaPaineis,
  focoAposFechar,
  opcoesDeConversa,
  opcoesDeConviteVazio,
  painelContemOpcao,
  rotuloDoPainel,
  type CandidatoFonte,
} from './quadro-helpers.ts'

const t = tradutor('pt-BR')
const escopoPessoal: LadoALadoEscopo | null = null
const escopoSpace: LadoALadoEscopo = { computerId: 'c1', hostId: 'h1', spaceId: 'sp1' }

const refA: LadoALadoRef = { computerId: 'c1', botId: 'bot-a' }
const refB: LadoALadoRef = { computerId: 'c1', botId: 'bot-b' }
const refC: LadoALadoRef = { computerId: 'c1', botId: 'bot-c' }
const refSpace: LadoALadoRef = { computerId: 'c1', botId: 'bot-s', spaceId: 'sp1', hubId: 'h1' }

function fontesExemplo(): CandidatoFonte<{ id: string; name: string }>[] {
  return [
    {
      ref: refA,
      botId: 'bot-a',
      botName: 'Alfa',
      bot: { id: 'bot-a', name: 'Alfa' },
      sessoesAbertas: [
        { id: 's-1', title: 'Pesquisa' },
        { id: 's-2', title: '' },
      ],
    },
    {
      ref: refB,
      botId: 'bot-b',
      botName: 'Beta',
      bot: { id: 'bot-b', name: 'Beta' },
      sessoesAbertas: [],
    },
  ]
}

test('opcoesDeConversa lista conversa principal e sessões extras abertas do bot', () => {
  const opcoes = opcoesDeConversa(fontesExemplo())
  assert.equal(opcoes.length, 4)
  assert.deepEqual(
    opcoes.map((o) => ({ bot: o.botName, sessionId: o.sessionId, principal: o.principal, temExtras: o.temExtras })),
    [
      { bot: 'Alfa', sessionId: null, principal: true, temExtras: true },
      { bot: 'Alfa', sessionId: 's-1', principal: false, temExtras: true },
      { bot: 'Alfa', sessionId: 's-2', principal: false, temExtras: true },
      { bot: 'Beta', sessionId: null, principal: true, temExtras: false },
    ],
  )
})

test('opcoesDeConviteVazio exclui conversas já abertas e limita a 6', () => {
  const fontes: CandidatoFonte<null>[] = Array.from({ length: 8 }, (_, idx) => ({
    ref: { computerId: 'c1', botId: `b-${idx}` },
    botId: `b-${idx}`,
    botName: `Bot ${idx}`,
    bot: null,
    sessoesAbertas: [],
  }))
  const paineis: LadoALadoPainel[] = [{ id: 'p1', ref: { computerId: 'c1', botId: 'b-0' }, sessionId: null }]
  const convite = opcoesDeConviteVazio(fontes, paineis)
  assert.equal(convite.length, 6)
  assert.equal(convite.some((f) => f.botId === 'b-0'), false)
  assert.equal(convite[0]?.botId, 'b-1')
})

test('painelContemOpcao distingue sessão principal de sessão extra do mesmo bot', () => {
  const paineis: LadoALadoPainel[] = [
    { id: 'p1', ref: refA, sessionId: null },
    { id: 'p2', ref: refA, sessionId: 's-1' },
  ]
  assert.equal(painelContemOpcao(paineis, refA, null), true)
  assert.equal(painelContemOpcao(paineis, refA, 's-1'), true)
  assert.equal(painelContemOpcao(paineis, refA, 's-2'), false)
  assert.equal(painelContemOpcao(paineis, refB, null), false)
})

test('acaoSelecaoBarraLateral foca existente, adiciona quando há 1 painel e troca o focado com 2+', () => {
  const umPainel: LadoALadoEscopoEstado = {
    ativo: true,
    focoId: 'p1',
    paineis: [{ id: 'p1', ref: refA, sessionId: null }],
  }
  assert.deepEqual(acaoSelecaoBarraLateral(umPainel, refA, escopoPessoal), { kind: 'focar', panelId: 'p1' })
  assert.deepEqual(acaoSelecaoBarraLateral(umPainel, refB, escopoPessoal), { kind: 'adicionar', ref: refB })

  const doisPaineis: LadoALadoEscopoEstado = {
    ativo: true,
    focoId: 'p1',
    paineis: [
      { id: 'p1', ref: refA, sessionId: null },
      { id: 'p2', ref: refB, sessionId: null },
    ],
  }
  assert.deepEqual(acaoSelecaoBarraLateral(doisPaineis, refB, escopoPessoal), { kind: 'focar', panelId: 'p2' })
  assert.deepEqual(acaoSelecaoBarraLateral(doisPaineis, refC, escopoPessoal), { kind: 'trocar', panelId: 'p1', ref: refC })

  // Quando o quadro está fechado ou a conversa pertence a outro escopo, não age.
  assert.deepEqual(acaoSelecaoBarraLateral({ ...umPainel, ativo: false }, refB, escopoPessoal), { kind: 'nenhuma' })
  assert.deepEqual(acaoSelecaoBarraLateral(umPainel, refSpace, escopoPessoal), { kind: 'nenhuma' })
  assert.deepEqual(
    acaoSelecaoBarraLateral({ ativo: true, focoId: 'p1', paineis: [{ id: 'p1', ref: refSpace, sessionId: null }] }, refA, escopoSpace),
    { kind: 'nenhuma' },
  )
})

test('rotuloDoPainel, focoAposFechar e anuncioMudancaPaineis resolvem rótulos e anúncios', () => {
  assert.equal(rotuloDoPainel('Alfa', null, null, t), 'Painel de Alfa')
  assert.equal(rotuloDoPainel('Alfa', 's-1', 'Pesquisa', t), 'Painel de Alfa · Pesquisa')

  const estado: LadoALadoEscopoEstado = {
    ativo: true,
    focoId: 'p2',
    paineis: [
      { id: 'p1', ref: refA, sessionId: null },
      { id: 'p2', ref: refB, sessionId: null },
    ],
  }
  assert.deepEqual(focoAposFechar(estado), refB)
  assert.deepEqual(focoAposFechar({ ...estado, focoId: 'inexistente' }), refA)

  const nomes = (ref: LadoALadoRef) => (ref.botId === 'bot-a' ? 'Alfa' : 'Beta')
  assert.equal(
    anuncioMudancaPaineis([{ id: 'p1', ref: refA, sessionId: null }], estado.paineis, nomes, t),
    'Beta adicionada ao quadro Lado a lado.',
  )
  assert.equal(
    anuncioMudancaPaineis(estado.paineis, [{ id: 'p1', ref: refA, sessionId: null }], nomes, t),
    'Painel de Beta fechado.',
  )
  assert.equal(
    anuncioMudancaPaineis([{ id: 'p1', ref: refA, sessionId: null }], [], nomes, t),
    '',
  )
})
