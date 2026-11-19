import assert from 'node:assert/strict'
import test from 'node:test'
import { CatalogoStore } from '../catalogo-skills/catalogo-store.ts'
import type { SkillClient } from '../../../../shared/fork-skills.ts'
import { contextoDoRender, origemDoRender, prepararExame, somentePessoal, type FonteRascunho } from './origem.ts'
import { textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// D454 (fatia 8b): a loja guarda frases; o texto pt-BR, sem os isolantes bidi dos nomes, é o de antes.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : '')


const rascunho = { name: 'Notas', description: 'Ao organizar notas.', instructions: 'Leia e organize.' }
function fixture() {
  const chamadas: [string, unknown][] = []
  const client: SkillClient = { async call<T>(m: string, b?: unknown): Promise<T> { chamadas.push([m, b]); return { skillProtocol: 2, items: [] } as T } }
  const fonte: FonteRascunho = { hello: { hostId: 'a' }, connectedClient: client, principalEpoch: 1, spaces: { personalSpaceId: 'pessoal' } }
  let ready: () => Promise<SkillClient> = async () => fonte.connectedClient!
  const loja = new CatalogoStore(() => ready(), () => {}, () => ({ hub: fonte.hello?.hostId ?? null, client: fonte.connectedClient, geracao: fonte.principalEpoch }))
  const examinar = (vista = origemDoRender(fonte), corpo = rascunho, space?: string) => {
    const contexto = contextoDoRender(fonte, loja, space)
    return prepararExame(contexto, contexto, vista, corpo)
  }
  return { fonte, loja, chamadas, examinar, ready: (f: typeof ready) => { ready = f } }
}

test('examinar inicializa catálogo do mesmo Hub, captura cópia e cancelar não grava', async () => {
  const f = fixture()
  const corpo = { ...rascunho }
  const sessao = f.examinar(origemDoRender(f.fonte), corpo)!
  corpo.instructions = 'texto posterior'
  assert.equal(sessao.rascunho.instructions, rascunho.instructions)
  assert.ok(f.loja.conexaoAtual(sessao.origem))
  await f.loja.carregar()
  assert.ok(f.chamadas.length)
  assert.ok(f.chamadas.every(([m]) => m === 'skills' || m === 'marketSkills'))
})

test('Space explícito desconhecido ou diferente não cai no Pessoal nem consulta catálogo', async () => {
  const f = fixture()
  for (const space of ['loja', 'desconhecido']) {
    assert.equal(somentePessoal(f.fonte, space), false)
    assert.equal(f.examinar(origemDoRender(f.fonte), rascunho, space), null)
  }
  f.fonte.spaceId = 'loja'
  assert.equal(f.examinar(), null)
  await Promise.resolve()
  assert.deepEqual(f.chamadas, [])
})

test('callback retido antes de examinar: outro Hub, A→B→A ou nova geração não sincronizam nem leem', async () => {
  const f = fixture()
  const vista = origemDoRender(f.fonte)
  f.fonte.hello = { hostId: 'b' }
  assert.equal(f.examinar(vista), null)
  f.fonte.hello = { hostId: 'a' }
  f.fonte.principalEpoch += 2
  assert.equal(f.examinar(vista), null)
  await Promise.resolve()
  assert.deepEqual(f.chamadas, [])
})

test('origem capturada ao examinar é recusada ao salvar depois de troca e durante ready', async () => {
  const f = fixture()
  const sessao = f.examinar()!
  await f.loja.carregar()
  f.fonte.principalEpoch++
  assert.equal(await f.loja.escrever(sessao.rascunho, sessao.origem), 'falhou')
  const nova = f.examinar()!
  let liberar!: (c: SkillClient) => void
  f.ready(() => new Promise((r) => { liberar = r }))
  const pedido = f.loja.escrever(nova.rascunho, nova.origem)
  f.fonte.principalEpoch++
  liberar(f.fonte.connectedClient!)
  // A reconciliação também usa ready; não a deixe pendurada na fixture.
  f.ready(async () => f.fonte.connectedClient!)
  assert.equal(await pedido, 'falhou')
  assert.ok(f.chamadas.every(([m]) => m === 'skills' || m === 'marketSkills'))
  assert.match(pt(f.loja.falhas.get('escrever')), /conexão com o Hub mudou/)
})

test('instâncias: tuplas idênticas não re-pareiam fonte/loja; ida e volta invalida token anterior', () => {
  const a = fixture()
  const fonteB = { ...a.fonte }
  const lojaB = new CatalogoStore(async () => fonteB.connectedClient!)
  const contextoA = contextoDoRender(a.fonte, a.loja)
  const vista = origemDoRender(a.fonte)
  assert.equal(prepararExame(contextoA, null, vista, rascunho), null, 'ponte desmontada')
  const contextoB = contextoDoRender(fonteB, lojaB, undefined, contextoA)
  assert.deepEqual(origemDoRender(fonteB), vista)
  assert.equal(prepararExame(contextoA, contextoB, vista, rascunho), null)
  const volta = contextoDoRender(a.fonte, a.loja, undefined, contextoB)
  assert.equal(prepararExame(contextoA, volta, vista, rascunho), null)
  const outraLoja = contextoDoRender(a.fonte, lojaB, undefined, contextoA)
  assert.equal(prepararExame(contextoA, outraLoja, vista, rascunho), null)
  a.fonte.spaceId = 'loja'
  const space = contextoDoRender(a.fonte, a.loja, undefined, contextoA)
  a.fonte.spaceId = undefined
  const pessoal = contextoDoRender(a.fonte, a.loja, undefined, space)
  assert.equal(prepararExame(contextoA, pessoal, vista, rascunho), null)
  assert.deepEqual(a.chamadas, [])
})
