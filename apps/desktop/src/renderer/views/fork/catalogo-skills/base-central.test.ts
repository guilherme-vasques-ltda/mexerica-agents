import assert from 'node:assert/strict'
import test from 'node:test'
import { captureSkillSelection, captureSkillSelectionOrigin, sendSkillSelection, type SkillSelectionClient } from '../../../../shared/fork-skill-selection.ts'
import { linhasDaMatriz, MarcasMatriz, textoInverso, TEXTOS_MATRIZ, type BotDaMatriz, type Celula, type OrigemMatriz, type SkillDaMatriz } from './base-central.ts'
import { textoDe, cru, type Frase } from './textos.ts'

// D70 (c2): só o fluxo da matriz — a célula projeta `cfg.skills` sem Não falso, um gesto por bot,
// recusa e resultado incerto sem reenvio, troca de conexão descarta, e só leitura onde o c1 não muda.

const V1 = 'a'.repeat(32)
const V2 = 'b'.repeat(32)
const NS = '11111111-1111-4111-8111-111111111111'
const skill = (id: string, contentStatus: SkillDaMatriz['contentStatus'] = 'verified'): SkillDaMatriz =>
  ({ id, name: id, description: `sobre ${id}`, installationId: `${NS}:1`, contentStatus })
const bot = (id: string, skills: string[], version: string | null = V1): BotDaMatriz =>
  ({ id, name: id, skills, skillSelection: version ? { protocol: 1, version } : null })
const origem = (geracao = 1, cliente: unknown = {}): OrigemMatriz => ({ hub: 'hub-a', cliente, geracao })
const valores = (c: Celula) => c.tipo === 'indisponivel' ? 'indisp' : `${c.tipo}:${c.ligada ? 'Sim' : 'Não'}`

test('célula projeta só cfg.skills; bot sem carimbo c1 fica indisponível, nunca Não', () => {
  const m = new MarcasMatriz()
  const [linha] = linhasDaMatriz([skill('notas')], [bot('donna', ['notas']), bot('x', []), bot('antigo', [], null)], m, 'vivo')
  assert.deepEqual(linha!.celulas.map(valores), ['controle:Sim', 'controle:Não', 'indisp'])
  assert.equal(linha!.ligadas, 1)
  assert.equal(linha!.semConfirmacao, 1)
  // Sem total categórico quando há célula sem confirmação.
  assert.equal(textoDe(textoInverso(linha!), 'pt-BR'), 'Confirmada em 1 bot · 1 sem confirmação')
  const [completa] = linhasDaMatriz([skill('notas')], [bot('donna', ['notas']), bot('x', [])], m, 'vivo')
  assert.equal(textoDe(textoInverso(completa!), 'pt-BR'), 'Ligada em 1 de 2 bots')
})

test('arquivo indisponível, instalação não conferida e órfã: vínculo em texto, sem controle e sem instalação inventada', () => {
  const m = new MarcasMatriz()
  const bots = [bot('donna', ['quebrada', 'sumida']), bot('x', [])]
  const linhas = linhasDaMatriz([skill('quebrada', 'unavailable'), skill('estranha', 'outra' as never)], bots, m, 'vivo')
  assert.deepEqual(linhas.map((l) => [l.id, l.aviso, l.skill === null]), [['quebrada', 'indisponivel', false], ['estranha', 'naoConferida', false], ['sumida', 'orfa', true]])
  assert.deepEqual(linhas[0]!.celulas.map(valores), ['texto:Sim', 'texto:Não'])
  assert.deepEqual(linhas[2]!.celulas, [{ tipo: 'texto', ligada: true, motivo: TEXTOS_MATRIZ.orfaLigada }, { tipo: 'texto', ligada: false, motivo: null }])
  // Órfã só de bot sem carimbo não vira linha: a lista dele não é tratada como conhecida.
  assert.equal(linhasDaMatriz([], [bot('antigo', ['x'], null)], m, 'vivo').length, 0)
})

test('texto alterado: desligar segue o c1, ligar fica travado; leitura guardada é só texto', () => {
  const m = new MarcasMatriz()
  const [l] = linhasDaMatriz([skill('mudou', 'changed')], [bot('donna', ['mudou']), bot('x', [])], m, 'vivo')
  assert.deepEqual(l!.celulas, [
    { tipo: 'controle', ligada: true, travada: null, recusa: null },
    { tipo: 'controle', ligada: false, travada: TEXTOS_MATRIZ.ligarAlterada, recusa: null },
  ])
  const [g] = linhasDaMatriz([skill('notas')], [bot('donna', ['notas'])], m, 'guardado')
  assert.deepEqual(g!.celulas, [{ tipo: 'texto', ligada: true, motivo: null }])
})

test('um gesto por bot: as outras células dele travam; valor só muda quando o mirror traz a versão nova', () => {
  const m = new MarcasMatriz()
  const o = origem()
  m.sincronizar(o)
  const x = bot('x', [])
  assert.equal(m.iniciar(o, x, 'a', true), true)
  assert.equal(m.iniciar(o, x, 'b', true), false, 'segundo gesto no mesmo bot recusado localmente')
  const linhas = () => linhasDaMatriz([skill('a'), skill('b')], [x, bot('y', [])], m, 'vivo')
  assert.deepEqual(linhas()[0]!.celulas[0], { tipo: 'salvando', ligada: false, pedido: true })
  assert.equal((linhas()[1]!.celulas[0] as { travada: unknown }).travada, TEXTOS_MATRIZ.outraPendente)
  assert.deepEqual(linhas()[1]!.celulas[1], { tipo: 'controle', ligada: false, travada: null, recusa: null }, 'outro bot livre')
  m.terminar(o, 'x', 'a', { ok: true })
  // Resposta chegou, mirror ainda na versão antiga: continua salvando, sem Sim otimista.
  assert.equal(linhasDaMatriz([skill('a')], [x], m, 'vivo')[0]!.celulas[0]!.tipo, 'salvando')
  const confirmado = bot('x', ['a'], V2)
  assert.deepEqual(linhasDaMatriz([skill('a')], [confirmado], m, 'vivo')[0]!.celulas[0], { tipo: 'controle', ligada: true, travada: null, recusa: null })
})

test('recusa e resultado incerto não reenviam; incerto só sai por Conferir na mesma conexão ou versão nova', async () => {
  const m = new MarcasMatriz()
  const o = origem()
  m.sincronizar(o)
  const x = bot('x', [])
  m.iniciar(o, x, 'a', true)
  m.terminar(o, 'x', 'a', { erro: new Error('As skills deste bot mudaram em outro lugar.'), mensagem: cru('As skills deste bot mudaram em outro lugar.') })
  // A recusa fica até o próximo gesto, mesmo que o evento traga outra versão.
  const depois = bot('x', [], V2)
  const celula = linhasDaMatriz([skill('a')], [depois], m, 'vivo')[0]!.celulas[0] as { tipo: string; ligada: boolean; travada: unknown; recusa: Frase | null }
  assert.deepEqual({ ...celula, recusa: textoDe(celula.recusa, 'pt-BR') }, { tipo: 'controle', ligada: false, travada: null, recusa: 'As skills deste bot mudaram em outro lugar.' })

  // Ler de novo só tira o aviso na mesma conexão; nada é reenviado.
  m.dispensarRecusa(origem(9), 'x', 'a')
  assert.equal((linhasDaMatriz([skill('a')], [depois], m, 'vivo')[0]!.celulas[0] as { recusa: string | null }).recusa !== null, true)
  m.dispensarRecusa(o, 'x', 'a')
  assert.equal((linhasDaMatriz([skill('a')], [depois], m, 'vivo')[0]!.celulas[0] as { recusa: string | null }).recusa, null)

  // Resultado incerto real do c1: resposta perdida no envio.
  let enviados = 0
  const client: SkillSelectionClient = { call: async () => { enviados++; throw new Error('socket hang up') } }
  const fonte = { hello: { hostId: 'hub-a' }, connectedClient: client, principalEpoch: 3, ready: async () => client }
  const c1 = captureSkillSelectionOrigin(fonte)
  const pedido = captureSkillSelection(c1, depois, skill('a'), true)
  const o2: OrigemMatriz = { hub: 'hub-a', cliente: client, geracao: 3 }
  m.sincronizar(o2)
  assert.equal(m.iniciar(o2, depois, 'a', true), true)
  const erro = await sendSkillSelection(c1, pedido).then(() => null, (e: unknown) => new Error('Não deu para confirmar.', { cause: e }))
  m.terminar(o2, 'x', 'a', { erro, mensagem: cru('Não deu para confirmar.') })
  assert.equal(enviados, 1)
  const [l] = linhasDaMatriz([skill('a')], [depois], m, 'vivo')
  assert.deepEqual(l!.celulas[0], { tipo: 'incerta', ligada: false })
  // D507 (#580): o zero tem forma natural nos 6 idiomas (`skills.matriz.bots.zero`), em vez de "0 bot".
  assert.equal(textoDe(textoInverso(l!), 'pt-BR'), 'Confirmada em nenhum bot · 1 sem confirmação')
  assert.equal(textoDe(textoInverso(l!), 'en'), 'Confirmed for no bots · 1 unconfirmed')
  assert.match(textoDe(textoInverso(l!), 'ar-AE'), /لا بوتات/)
  m.conferir(origem(9), 'x', 'a')
  assert.equal(linhasDaMatriz([skill('a')], [depois], m, 'vivo')[0]!.celulas[0]!.tipo, 'incerta', 'Conferir de outra conexão não limpa')
  m.conferir(o2, 'x', 'a')
  assert.equal(linhasDaMatriz([skill('a')], [depois], m, 'vivo')[0]!.celulas[0]!.tipo, 'controle')
  assert.equal(enviados, 1, 'nada reenviado')
})

test('troca de conexão (reconexão ou A→B→A) descarta marcas e avisa; resposta antiga não volta a valer', () => {
  const m = new MarcasMatriz()
  const cliente = {}
  const a = origem(1, cliente)
  m.sincronizar(a)
  const x = bot('x', [])
  m.iniciar(a, x, 'a', true)
  m.sincronizar(origem(2, cliente))
  assert.equal(m.aviso, TEXTOS_MATRIZ.conexaoMudou)
  assert.equal(m.salvandoNoBot(x), false)
  m.terminar(a, 'x', 'a', { ok: true })
  assert.equal(m.marca('x', 'a'), null, 'resposta da conexão anterior ignorada')
  assert.equal(m.iniciar(a, x, 'a', true), false, 'gesto com origem velha não começa')
})
