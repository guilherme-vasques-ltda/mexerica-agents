import assert from 'node:assert/strict'
import test from 'node:test'
import type { ChaveAparelho, EstadoChaves } from '../../../../shared/fork-voz.ts'
import { agirNoAparelho, fonteEfetiva, guardarNoAparelho, textoDoResultado, textoTransicaoMac, textoTransicaoSemLocal } from './chave-aparelho.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { textosVoz } from './frases.ts'

// Fatia 6b (D454, D507): os textos são dado; o teste confere a frase pt-BR que a tela mostra.
const pt = (partes: Mensagem[], mac = true) => textosVoz(tradutor('pt-BR'), partes, undefined, mac)

// voz-04 (D70): ordem das fontes por modo, textos aprovados da transição e o campo da chave.

const CHAVE = 'sk-proj-FICTICIA0123456789abcdefghijklmnopqrstuvwxyz'
const c = (situacao: ChaveAparelho['situacao'], reserva = false): ChaveAparelho => ({ situacao, reserva })
const estado: EstadoChaves = { cofre: 'seguro', chaves: { openai: c('guardada'), gemini: c('ausente') } }

test('ordem das fontes: a chave deste aparelho, depois a do Hub, depois sem chave', () => {
  assert.equal(fonteEfetiva('tempo-real', c('guardada'), 'hub'), 'aparelho', 'a própria vence a do Hub')
  assert.equal(fonteEfetiva('tempo-real', c('ausente'), 'hub'), 'hub')
  assert.equal(fonteEfetiva('tempo-real', null, 'nenhuma'), 'nenhuma')
  assert.equal(fonteEfetiva('tempo-real', null, 'desconhecida'), 'desconhecida')
  // Travada ou recusada não é ausente: nunca passa para o Hub sem escolha.
  assert.equal(fonteEfetiva('tempo-real', c('travada'), 'hub'), 'nao-abriu')
  assert.equal(fonteEfetiva('tempo-real', c('recusada'), 'hub'), 'nao-abriu')
  assert.equal(fonteEfetiva('tempo-real', c('travada', true), 'hub'), 'hub', 'só com a escolha da pessoa')
  assert.equal(fonteEfetiva('tempo-real', c('guardada', true), 'hub'), 'hub', 'a escolha continua se o cofre voltar a abrir')
})

test('até o 05b, a chave deste aparelho nunca deixa o Hub pagar no Fala, em "ouvir" e em "ler" separados', () => {
  for (const modo of ['ouvir', 'ler'] as const) {
    for (const s of ['guardada', 'travada', 'recusada'] as const) assert.equal(fonteEfetiva(modo, c(s), 'hub'), 'so-tempo-real', `${modo}/${s}`)
    assert.equal(fonteEfetiva(modo, c('ausente'), 'hub'), 'hub', `${modo}: sem chave daqui, a reserva continua`)
  }
})

test('textos aprovados da transição: Mac por parte; Windows/Linux com a segunda saída só se o Hub tem chave', () => {
  assert.equal(pt([{ chave: textoTransicaoMac('ouvir') }]), 'A chave deste Mac ainda funciona só no Tempo real do Ao vivo. Seguindo com o Ditado do macOS.')
  assert.equal(pt([{ chave: textoTransicaoMac('ler') }]), 'A chave deste Mac ainda funciona só no Tempo real do Ao vivo. Seguindo com a Voz do macOS.')
  assert.equal(pt([{ chave: textoTransicaoMac('ambos') }]), 'A chave deste Mac ainda funciona só no Tempo real do Ao vivo. Seguindo com o Ditado e a Voz do macOS.')
  assert.equal(pt([textoTransicaoSemLocal('servidor')], false), 'A chave deste computador ainda funciona só no Tempo real do Ao vivo. Para o modo Fala, use o Tempo real ou limpe a chave deste computador para usar a do computador principal.')
  assert.equal(pt([textoTransicaoSemLocal(null)], false), 'A chave deste computador ainda funciona só no Tempo real do Ao vivo. Para o modo Fala, use o Tempo real.')
})

test('o campo da chave é limpo antes da resposta, e nenhum texto devolve a chave', async () => {
  let campo = `  ${CHAVE} `
  let recebida = ''
  let campoNaChamada: string | null = null
  const r = await guardarNoAparelho({ guardar: async (_p, k) => { recebida = k; campoNaChamada = campo; return { ok: true, categoria: null, estado } } }, 'openai', campo, () => { campo = '' })
  assert.equal(campoNaChamada, '', 'vazio antes de o main responder')
  assert.equal(recebida, CHAVE, 'vai aparada, uma vez')
  assert.equal(pt(r.partes), 'A chave funciona e ficou guardada com segurança neste Mac.')
  // A ponte que lança um erro com a chave (o que nunca deveria) não faz a chave aparecer.
  const falhou = await guardarNoAparelho({ guardar: async () => { throw new Error(`Incorrect API key provided: ${CHAVE}`) } }, 'openai', CHAVE, () => {})
  assert.equal(falhou.ok, false)
  assert.doesNotMatch(pt(falhou.partes), /sk-|FICTICIA/)
  for (const categoria of ['chave', 'saldo', 'limite', 'rede', 'provedor', 'cofre', 'registro', 'cancelado', 'sem-cofre', 'sem-chave', 'invalida', 'origem'] as const) {
    for (const acao of ['guardar', 'limpar', 'testar', 'reserva'] as const) assert.doesNotMatch(pt(textoDoResultado(acao, { ok: false, categoria }, 'servidor').partes, true), /sk-|FICTICIA/)
  }
  assert.equal(pt(textoDoResultado('guardar', { ok: false, categoria: 'chave' }, null).partes, true), 'O provedor recusou a chave. Nada foi guardado.')
  assert.equal(pt(textoDoResultado('guardar', { ok: false, categoria: 'sem-cofre' }, null).partes, false), 'Este computador não tem um cofre seguro para guardar chaves; a voz usa a chave do computador principal. Nada foi gravado.')
  assert.equal(pt(textoDoResultado('limpar', { ok: false, categoria: 'cofre' }, null).partes, true), 'A chave está sendo apagada. Tente Limpar de novo.')
  const limpo = await agirNoAparelho({ limpar: async () => ({ ok: true, categoria: null, estado }), testar: async () => { throw new Error() }, usarReserva: async () => { throw new Error() } }, 'limpar', 'openai', 'servidor')
  assert.equal(pt(limpo.partes), 'Chave apagada neste Mac. A voz passa a usar a chave do computador principal.')
})

test('estados de disco pendentes bloqueiam a fonte em todos os modos e têm texto próprio', () => {
  for (const situacao of ['incerta', 'limpeza-pendente'] as const) {
    for (const modo of ['tempo-real', 'ouvir', 'ler'] as const) {
      assert.equal(fonteEfetiva(modo, c(situacao), 'hub'), situacao)
      assert.equal(fonteEfetiva(modo, c(situacao, true), 'hub'), 'hub', 'somente escolha explícita')
    }
    for (const acao of ['guardar', 'limpar', 'testar'] as const) {
      assert.equal(pt(textoDoResultado(acao, { ok: false, categoria: 'cofre' }, 'servidor', situacao).partes, true),
        situacao === 'incerta' ? 'Não foi possível confirmar se a chave foi guardada neste Mac.' : 'A chave está sendo apagada. Tente Limpar de novo.')
    }
  }
})
