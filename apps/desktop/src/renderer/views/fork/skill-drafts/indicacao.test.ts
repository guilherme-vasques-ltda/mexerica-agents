import assert from 'node:assert/strict'
import test from 'node:test'
import { extrairProposta, extrairRascunho, LIMITES_RASCUNHO } from './rascunho.ts'
import { CatalogoStore } from '../catalogo-skills/catalogo-store.ts'
import { capturarExame, contextoDoRender, origemDoRender, type FonteRascunho } from './origem.ts'
import { textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// D454 (fatia 8b): a loja guarda frases; o texto pt-BR, sem os isolantes bidi dos nomes, é o de antes.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : '')


const corpo = { schema: 1, reason: 'Este procedimento se repete.', reference: 'organizar-notas' }
const bloco = (v: unknown = corpo, fence = 'reference') => '```donna-skill-' + fence + '\n' + JSON.stringify(v) + '\n```'

test('b2: contrato exato, Unicode, limites e Markdown sem interpretação da referência', () => {
  assert.deepEqual(extrairProposta(bloco().replaceAll('\n', '\r\n')), { tipo: 'indicacao', reason: corpo.reason, reference: corpo.reference })
  assert.equal(extrairRascunho(bloco()), null)
  assert.ok(extrairProposta(bloco({ ...corpo, reason: '😀'.repeat(1024), reference: '😀'.repeat(256) })))
  // URL/path continuam somente strings de busca, sem conversão em identidade/importação.
  for (const reference of ['https://example.test/x', '/tmp/SKILL.md']) assert.equal(extrairProposta(bloco({ ...corpo, reference }))?.tipo, 'indicacao')
  for (const v of [{ ...corpo, schema: '1' }, { ...corpo, reason: ' ' }, { ...corpo, reference: '' }, { ...corpo, reason: 2 }, { ...corpo, reference: [] }, { ...corpo, author: 'Donna' }, { ...corpo, reason: '😀'.repeat(1025) }, { ...corpo, reference: '😀'.repeat(257) }]) assert.equal(extrairProposta(bloco(v)), null)
  assert.equal(extrairProposta(bloco().replace('"reference":', '"refere\\u006ece":"duplicada","reference":')), null)
  assert.equal(extrairProposta('```donna-skill-reference\n' + ' '.repeat(LIMITES_RASCUNHO.envelope) + JSON.stringify(corpo) + '\n```'), null)
  assert.equal(extrairProposta('á'.repeat(65536) + '\n' + bloco()), null)
})

test('b2: um único extrator para os três fences, contando inválidos/incompletos; citações passivas', () => {
  const draft = { schema: 1, name: 'Notas', description: 'Ao revisar.', instructions: 'Leia.' }
  const propostas = [bloco(), bloco(draft, 'draft'), bloco({ ...draft, reason: 'Repete.' }, 'recommendation')]
  for (const a of propostas) for (const b of [...propostas, bloco({ ...corpo, approval: true }), bloco().slice(0, -3)]) assert.equal(extrairProposta(a + '\n' + b), null)
  for (const prefixo of ['> ', '    ', '  ']) assert.equal(extrairProposta(bloco().split('\n').map((l) => prefixo + l).join('\n')), null)
  for (const s of ['````markdown\n' + bloco() + '\n````', bloco().slice(0, -3), bloco().replace('reference', 'reference extra')]) assert.equal(extrairProposta(s), null)
})

test('b2: captura comum sem rascunho nem carga antecipada; outro Space, clone e offline não esperam ready', async () => {
  let prontidoes = 0
  const fonte: FonteRascunho = { hello: { hostId: 'a' }, connectedClient: null, principalEpoch: 1, spaces: { personalSpaceId: 'pessoal' } }
  const loja = new CatalogoStore(async () => { prontidoes++; throw new Error('não esperar') }, () => {}, () => ({ hub: fonte.hello!.hostId, client: fonte.connectedClient, geracao: fonte.principalEpoch }))
  const contexto = contextoDoRender(fonte, loja)
  const captura = capturarExame(contexto, contexto, origemDoRender(fonte), true)!
  assert.ok(captura)
  assert.equal('rascunho' in captura, false)
  assert.equal(prontidoes, 0)
  await loja.carregar({ origem: captura.origem, valida: () => true })
  assert.equal(prontidoes, 0)
  assert.match(pt(loja.erro), /Sem conexão/)
  await assert.rejects(loja.detalhes({ id: 'nota' }, captura.origem, { origem: captura.origem, valida: () => true }), (e: unknown) => /Sem conexão/.test(pt((e as { frase?: Frase }).frase)))
  assert.equal(prontidoes, 0)
  const clone = contextoDoRender({ ...fonte }, loja)
  assert.equal(capturarExame(contexto, clone, origemDoRender(fonte), true), null)
  const space = contextoDoRender(fonte, loja, 'outro')
  assert.equal(capturarExame(space, space, origemDoRender(fonte), true), null)
})
