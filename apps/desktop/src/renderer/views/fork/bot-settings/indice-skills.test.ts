import assert from 'node:assert/strict'
import test from 'node:test'
import type { FonteSkills } from '../catalogo-skills/consumidores.ts'
import { estadoDoIndice, formatarTamanho, FORMATO_ANTIGO, lerSkillsDoBot, normalizarIndice, textoDoIndice } from './indice-skills.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'

const t = tradutor('pt-BR')
const texto = (e: ReturnType<typeof estadoDoIndice>) => (e.tipo === 'medido' ? textoDoIndice(t, 'pt-BR', e.indice, e.lidoEm) : null)

// R3: a medida do Hub só aparece como medida da seleção na tela; ausente, inválida, de outra
// origem ou de outra versão de seleção nunca vira zero nem a medida da seleção mostrada.

const V1 = 'a'.repeat(32)
const V2 = 'b'.repeat(32)
type Fonte = { -readonly [K in keyof FonteSkills]: FonteSkills[K] }
const cliente = (responder: (method: string, body?: unknown) => unknown) => {
  const pedidos: [string, unknown][] = []
  return {
    pedidos,
    async call<T>(method: string, body?: unknown): Promise<T> {
      pedidos.push([method, body])
      return responder(method, body) as T
    },
  }
}
const fonte = (client: FonteSkills['connectedClient'], hostId = 'hub-a', epoch = 1): Fonte =>
  ({ hello: { hostId }, connectedClient: client, principalEpoch: epoch, ready: async () => client! })
const index = (enabled: number, listed: number, bytes: number, selectionVersion = V1) => ({ enabled, listed, bytes, selectionVersion })

test('normaliza só a medida coerente e com versão de seleção; o resto é indisponível', () => {
  assert.deepEqual(normalizarIndice(index(8, 6, 1234)), { indice: { ligadas: 8, noIndice: 6, bytes: 1234 }, versao: V1 })
  assert.deepEqual(normalizarIndice(index(3, 0, 0))?.indice, { ligadas: 3, noIndice: 0, bytes: 0 })
  for (const raw of [undefined, null, {}, index(2, 3, 9), index(1, 1, 0), index(1, 0, 5), index(-1, 0, 0), index(1.5, 0, 0),
    { enabled: 1, listed: 1, bytes: 9 }, index(1, 1, 9, 'curta'), index(1, 1, 9, 'A'.repeat(32))]) {
    assert.equal(normalizarIndice(raw), null, JSON.stringify(raw))
  }
})

test('tamanho em bytes abaixo de 1.000 e KB de 1.000 bytes com uma casa', () => {
  assert.deepEqual([0, 104, 999, 1000, 1234, 1250, 12_345].map((b) => formatarTamanho('pt-BR', b)),
    ['0 KB', '104 bytes', '999 bytes', '1,0 KB', '1,2 KB', '1,3 KB', '12,3 KB'])
})

test('medida só vale na origem viva e na versão de seleção que o Hub mediu', async () => {
  const c = cliente(() => ({ items: [], index: index(2, 1, 1234) }))
  const f = fonte(c)
  const { leitura } = await lerSkillsDoBot(f, c, 'bot-1', V1)
  const medido = estadoDoIndice(f, leitura, V1)
  assert.equal(texto(medido)?.linha, '2 ligadas · 1 no índice · 1,2 KB no índice')
  assert.match(texto(medido)?.detalhe ?? '', /^1\.234 bytes, lidos às \d\d:\d\d$/)
  assert.equal(medido.tipo === 'medido' && textoDoIndice(tradutor('en'), 'en', medido.indice, medido.lidoEm).linha, '2 on · 1 in the index · 1.2 KB in the index')
  assert.deepEqual(estadoDoIndice(f, leitura, V2), { tipo: 'conferindo' }, 'tela já em outra seleção')
  assert.deepEqual(estadoDoIndice(f, leitura, null), { tipo: 'indisponivel' }, 'bot sem versão')
  for (const outra of [fonte(c, 'hub-b'), fonte(c, 'hub-a', 2), fonte(cliente(() => ({}))), fonte(null)]) {
    assert.deepEqual(estadoDoIndice(outra, leitura, V1), { tipo: 'indisponivel' })
  }
  // Pedido pela seleção V1, mas o Hub mediu outra (V2) com a mesma contagem: não vira medida de V1.
  const tardio = cliente(() => ({ items: [], index: index(2, 1, 1234, V2) }))
  const ft = fonte(tardio)
  assert.deepEqual(estadoDoIndice(ft, (await lerSkillsDoBot(ft, tardio, 'bot-1', V1)).leitura, V1), { tipo: 'conferindo' })
  // Link que não é o vivo: a medida já nasce indisponível.
  const velho = cliente(() => ({ items: [], index: index(1, 1, 200) }))
  assert.deepEqual(estadoDoIndice(f, (await lerSkillsDoBot(f, velho, 'bot-1', V1)).leitura, V1), { tipo: 'indisponivel' })
})

test('zero só quando o Hub confirma, e ligadas bloqueadas seguem contadas', async () => {
  for (const [raw, linha] of [[index(0, 0, 0), '0 ligadas · 0 no índice · 0 KB no índice'], [index(1, 0, 0), '1 ligada · 0 no índice · 0 KB no índice']] as const) {
    const c = cliente(() => ({ items: [], index: raw }))
    const f = fonte(c)
    const e = estadoDoIndice(f, (await lerSkillsDoBot(f, c, 'bot-1', V1)).leitura, V1)
    assert.equal(texto(e)?.linha, linha)
  }
  for (const raw of [null, undefined]) {
    const c = cliente(() => ({ items: [], index: raw }))
    const f = fonte(c)
    assert.deepEqual(estadoDoIndice(f, (await lerSkillsDoBot(f, c, 'bot-1', V1)).leitura, V1), { tipo: 'indisponivel' })
  }
})

test('só a recusa de formato na mesma origem relê sem botId; outras falhas não repetem a leitura', async () => {
  const antigo = cliente((_m, body) => {
    if (body !== undefined) throw new Error(FORMATO_ANTIGO)
    return { items: [{ id: 'a' }] }
  })
  const fa = fonte(antigo)
  const lido = await lerSkillsDoBot(fa, antigo, 'bot-1', V1)
  assert.deepEqual([lido.items, antigo.pedidos], [[{ id: 'a' }], [['skills', { botId: 'bot-1' }], ['skills', undefined]]])
  assert.deepEqual(estadoDoIndice(fa, lido.leitura, V1), { tipo: 'indisponivel' })

  for (const mensagem of ['forbidden', 'cross_space_reference', 'skills_personal_only', 'Bot não encontrado', "Can't reach your computer."]) {
    const c = cliente(() => { throw new Error(mensagem) })
    await assert.rejects(lerSkillsDoBot(fonte(c), c, 'bot-1', V1), { message: mensagem })
    assert.equal(c.pedidos.length, 1, mensagem)
  }
  // Origem trocada durante o pedido: nem a recusa de formato provoca segunda leitura.
  const f = fonte(null)
  const trocou = cliente(() => { f.principalEpoch = 2; throw new Error(FORMATO_ANTIGO) })
  f.connectedClient = trocou
  await assert.rejects(lerSkillsDoBot(f, trocou, 'bot-1', V1), { message: FORMATO_ANTIGO })
  assert.equal(trocou.pedidos.length, 1)
})
