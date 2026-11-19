import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  agruparExcluidos, alterarNovosBots, classificarFalha, definirNovosBots, interruptorInicial, lerNovosBots, proximoInterruptor, falhaDoInterruptor, lerPrevia, SessaoEmTodos, temInterruptorNovosBots,
  type Alvo, type Cliente, type Estado,
} from './em-todos.ts'

// MX-264: o modelo da tela "Ligar em todos os bots". O Hub é um cliente falso que responde na ordem combinada.
const alvo: Alvo = { tipo: 'skill', id: 'revisar-codigo', nome: 'Revisar código' }
const previaDoHub = (versao: string, recebedores: string[], excluidos: [string, string][] = []) => ({
  tipo: 'skill', id: alvo.id, versao,
  recebedores: recebedores.map((botId) => ({ botId, nome: `Bot ${botId}` })),
  excluidos: excluidos.map(([botId, motivo]) => ({ botId, motivo })),
})
class ErroHub extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

/** Respostas em fila por método; registra cada chamada (método e corpo). */
function hubFalso(respostas: Record<string, (unknown | Error)[]>) {
  const chamadas: { metodo: string; corpo: unknown }[] = []
  const cliente: Cliente = {
    async call<T>(metodo: string, corpo?: unknown) {
      chamadas.push({ metodo, corpo })
      const fila = respostas[metodo]
      assert.ok(fila && fila.length > 0, `chamada sem resposta combinada: ${metodo}`)
      const r = fila.shift()
      if (r instanceof Error) throw r
      return r as T
    },
  }
  return { cliente, chamadas }
}
/** O estado sem o estreitamento que o TypeScript faz depois de um assert. */
const estadoDe = (s: SessaoEmTodos): Estado => s.estado
const esperar = () => new Promise<void>((r) => setTimeout(r, 0))

test('prévia: lista quem recebe e quem fica de fora; o token volta ao Hub sem ser aberto', async () => {
  const { cliente, chamadas } = hubFalso({
    extensaoEmTodosPrevia: [previaDoHub('tok-opaco-1', ['a', 'b'], [['c', 'ja_tem']])],
    extensaoEmTodosAplicar: [{ aplicados: ['a', 'b'] }],
  })
  const sessao = new SessaoEmTodos(cliente, alvo)
  assert.equal(sessao.estado.fase, 'fechado')
  sessao.abrir()
  assert.equal(sessao.estado.fase, 'lendo')
  await esperar()
  const lida = estadoDe(sessao)
  assert.equal(lida.fase, 'previa')
  if (lida.fase !== 'previa') return
  assert.deepEqual(lida.previa.recebedores.map((r) => r.botId), ['a', 'b'])
  assert.deepEqual(lida.previa.excluidos, [{ botId: 'c', motivo: 'ja_tem' }])
  assert.equal(lida.mudou, false)
  assert.deepEqual(chamadas[0], { metodo: 'extensaoEmTodosPrevia', corpo: { protocol: 1, tipo: 'skill', id: 'revisar-codigo' } })

  await sessao.confirmar()
  assert.deepEqual(sessao.estado, { fase: 'feito', quantos: 2 })
  assert.deepEqual(chamadas[1], { metodo: 'extensaoEmTodosAplicar', corpo: { protocol: 1, tipo: 'skill', id: 'revisar-codigo', botIds: ['a', 'b'], versao: 'tok-opaco-1' } })
})

test('um OK só: confirmar duas vezes seguidas aplica uma vez; sem recebedores não há o que confirmar', async () => {
  const { cliente, chamadas } = hubFalso({
    extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])],
    extensaoEmTodosAplicar: [{ aplicados: ['a'] }],
  })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  const primeira = sessao.confirmar()
  const segunda = sessao.confirmar()
  await Promise.all([primeira, segunda])
  assert.equal(chamadas.filter((c) => c.metodo === 'extensaoEmTodosAplicar').length, 1)

  const vazio = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', [], [['a', 'ja_tem']])] })
  const s2 = new SessaoEmTodos(vazio.cliente, alvo)
  s2.abrir()
  await esperar()
  await s2.confirmar()
  assert.equal(s2.estado.fase, 'previa')
  assert.equal(vazio.chamadas.length, 1)
})

test('conflito: nada é reenviado; a tela refaz a prévia sozinha e avisa que a lista mudou', async () => {
  const { cliente, chamadas } = hubFalso({
    extensaoEmTodosPrevia: [previaDoHub('v1', ['a', 'b']), previaDoHub('v2', ['a', 'b', 'novo'])],
    extensaoEmTodosAplicar: [new ErroHub(409, 'extensoes_em_todos_conflito')],
  })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  await sessao.confirmar()
  await esperar()
  assert.deepEqual(chamadas.map((c) => c.metodo), ['extensaoEmTodosPrevia', 'extensaoEmTodosAplicar', 'extensaoEmTodosPrevia'])
  const refeitaLida = estadoDe(sessao)
  assert.equal(refeitaLida.fase, 'previa')
  if (refeitaLida.fase !== 'previa') return
  assert.equal(refeitaLida.mudou, true, 'a lista nova vem com o aviso')
  assert.equal(refeitaLida.previa.versao, 'v2')
  assert.deepEqual(refeitaLida.previa.recebedores.map((r) => r.botId), ['a', 'b', 'novo'])
  // Só um novo OK aplica, e com a lista e o token novos.
  const { cliente: c2, chamadas: ch2 } = hubFalso({ extensaoEmTodosAplicar: [{ aplicados: ['a', 'b', 'novo'] }] })
  const refeita = new SessaoEmTodos(c2, alvo)
  refeita.estado = refeitaLida
  await refeita.confirmar()
  assert.deepEqual((ch2[0]?.corpo as { botIds: string[]; versao: string }).botIds, ['a', 'b', 'novo'])
  assert.equal((ch2[0]?.corpo as { versao: string }).versao, 'v2')
  assert.deepEqual(refeita.estado, { fase: 'feito', quantos: 3 })
})

test('conflito seguido de conflito não vira laço: cada confirmar refaz a prévia uma vez e espera o OK', async () => {
  const { cliente, chamadas } = hubFalso({
    extensaoEmTodosPrevia: [previaDoHub('v1', ['a']), previaDoHub('v2', ['a']), previaDoHub('v3', ['a'])],
    extensaoEmTodosAplicar: [new ErroHub(409, 'extensoes_em_todos_conflito'), new ErroHub(409, 'extensoes_em_todos_conflito')],
  })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  await sessao.confirmar()
  await esperar()
  assert.equal(chamadas.filter((c) => c.metodo === 'extensaoEmTodosAplicar').length, 1)
  await sessao.confirmar()
  await esperar()
  assert.equal(chamadas.filter((c) => c.metodo === 'extensaoEmTodosAplicar').length, 2)
  assert.equal(sessao.estado.fase, 'previa')
})

test('permissão negada: a tela mostra o caso de permissão e não tenta de novo', async () => {
  for (const falha of [new ErroHub(403, 'forbidden'), new ErroHub(401, 'unauthorized'), new ErroHub(400, 'not_allowed')]) {
    const { cliente, chamadas } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [falha] })
    const sessao = new SessaoEmTodos(cliente, alvo)
    sessao.abrir()
    await esperar()
    await sessao.confirmar()
    assert.deepEqual(sessao.estado, { fase: 'erro', erro: 'permissao' })
    assert.equal(chamadas.length, 2, 'nada é repetido')
  }
  // A própria prévia pode ser recusada.
  const { cliente } = hubFalso({ extensaoEmTodosPrevia: [new ErroHub(403, 'forbidden')] })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  assert.deepEqual(sessao.estado, { fase: 'erro', erro: 'permissao' })
})

test('outras falhas: extensão não instalada, Hub antigo, link que caiu e erro comum', async () => {
  const casos: [Error, string][] = [
    [new ErroHub(409, 'extensoes_em_todos_indisponivel'), 'indisponivel'],
    [new ErroHub(404, 'unknown_method'), 'semFuncao'],
    [new ErroHub(0, "Can't reach your computer."), 'incerto'],
    [new Error('tempo esgotado'), 'incerto'],
    [new ErroHub(500, 'boom'), 'generico'],
  ]
  for (const [falha, esperado] of casos) {
    const { cliente } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [falha] })
    const sessao = new SessaoEmTodos(cliente, alvo)
    sessao.abrir()
    await esperar()
    await sessao.confirmar()
    assert.deepEqual(sessao.estado, { fase: 'erro', erro: esperado as never }, String(falha.message))
  }
})

test('resposta que o app não sabe ler: a prévia não aparece e a aplicação não é dada como feita', async () => {
  const ruim = hubFalso({ extensaoEmTodosPrevia: [{ ...previaDoHub('v1', ['a']), versao: '' }] })
  const s1 = new SessaoEmTodos(ruim.cliente, alvo)
  s1.abrir()
  await esperar()
  assert.deepEqual(s1.estado, { fase: 'erro', erro: 'generico' })

  const incerta = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [{ ok: true }] })
  const s2 = new SessaoEmTodos(incerta.cliente, alvo)
  s2.abrir()
  await esperar()
  await s2.confirmar()
  assert.deepEqual(s2.estado, { fase: 'erro', erro: 'incerto' })
})

test('trocar de Hub ou de conexão no meio descarta a resposta; fechar também', async () => {
  let viva = true
  const { cliente } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])] })
  const sessao = new SessaoEmTodos(cliente, alvo, () => viva)
  sessao.abrir()
  viva = false
  await esperar()
  assert.equal(sessao.estado.fase, 'fechado')

  const outra = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])] })
  const s2 = new SessaoEmTodos(outra.cliente, alvo)
  s2.abrir()
  s2.fechar()
  await esperar()
  assert.equal(s2.estado.fase, 'fechado', 'a resposta que chega depois de fechar não reabre o bloco')
})

test('aplicando: fechar não cancela o que já foi ao Hub, mas a resposta não reabre nada', async () => {
  const { cliente } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [{ aplicados: ['a'] }] })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  const pendente = sessao.confirmar()
  sessao.fechar()
  await pendente
  assert.equal(sessao.estado.fase, 'fechado')
})

test('lerPrevia só aceita a resposta desta extensão, com tudo no formato do contrato', () => {
  const certo = previaDoHub('v', ['a'], [['b', 'oculto']])
  assert.ok(lerPrevia(certo, alvo))
  assert.equal(lerPrevia({ ...certo, id: 'outra' }, alvo), null)
  assert.equal(lerPrevia({ ...certo, tipo: 'conector' }, alvo), null)
  assert.equal(lerPrevia({ ...certo, recebedores: [{ botId: 'a' }] }, alvo), null)
  assert.equal(lerPrevia({ ...certo, excluidos: [{ motivo: 'x' }] }, alvo), null)
  assert.equal(lerPrevia({ ...certo, recebedores: 'a' }, alvo), null)
  assert.equal(lerPrevia(null, alvo), null)
})

test('os de fora agrupam por motivo em ordem fixa, com o nome que o app conhece; motivo novo fica em "outro"', () => {
  const nomes: Record<string, string> = { a: 'Ana', b: 'Beto', c: 'Cris' }
  const grupos = agruparExcluidos([
    { botId: 'a', motivo: 'oculto' }, { botId: 'b', motivo: 'ja_tem' }, { botId: 'c', motivo: 'ja_tem' },
    { botId: 'x', motivo: 'apagado' }, { botId: 'y', motivo: 'motivo_do_futuro' },
  ], (id) => nomes[id] ?? null)
  assert.deepEqual(grupos, [
    { motivo: 'ja_tem', nomes: ['Beto', 'Cris'], semNome: 0 },
    { motivo: 'oculto', nomes: ['Ana'], semNome: 0 },
    { motivo: 'apagado', nomes: [], semNome: 1 },
    { motivo: 'outro', nomes: [], semNome: 1 },
  ])
  assert.deepEqual(agruparExcluidos([], () => null), [])
})

test('só skills têm o interruptor "Novos bots já vêm com esta skill"; conector nem chega ao Hub', async () => {
  assert.equal(temInterruptorNovosBots('skill'), true)
  assert.equal(temInterruptorNovosBots('conector'), false)

  const { cliente, chamadas } = hubFalso({ extensaoNovosBotsDefinir: [{ id: 'revisar-codigo', novosBots: true }, { id: 'revisar-codigo', novosBots: false }] })
  assert.equal(await definirNovosBots(cliente, alvo, true), true)
  assert.deepEqual(chamadas[0], { metodo: 'extensaoNovosBotsDefinir', corpo: { protocol: 1, tipo: 'skill', id: 'revisar-codigo', ligado: true } })
  assert.equal(await definirNovosBots(cliente, alvo, false), false, 'vale o que o Hub gravou')

  await assert.rejects(definirNovosBots(cliente, { tipo: 'conector', id: 'github' }, true))
  assert.equal(chamadas.length, 2, 'para conector nenhuma chamada sai')

  const estranha = hubFalso({ extensaoNovosBotsDefinir: [{ id: 'outra', novosBots: true }] })
  await assert.rejects(definirNovosBots(estranha.cliente, alvo, true), 'resposta de outra skill não vale')
})

test('falhas do interruptor: permissão, Hub antigo ou genérica', () => {
  assert.equal(falhaDoInterruptor(new ErroHub(403, 'forbidden')), 'permissao')
  assert.equal(falhaDoInterruptor(new ErroHub(404, 'unknown_method')), 'semFuncao')
  assert.equal(falhaDoInterruptor(new ErroHub(400, 'extensoes_novos_bots_recusado')), 'generico')
  assert.equal(falhaDoInterruptor(new ErroHub(500, 'boom')), 'generico')
})

test('classificarFalha: conflito vence qualquer status; sem status vira genérico (quem escreve decide o incerto)', () => {
  assert.equal(classificarFalha(new ErroHub(403, 'extensoes_em_todos_conflito')), 'conflito')
  assert.equal(classificarFalha(new Error('x')), 'generico')
  assert.equal(classificarFalha('texto solto'), 'generico')
  assert.equal(classificarFalha(null), 'generico')
})

test('origem trocada com a prévia na tela: confirmar não escreve nada (zero aplicar) e o bloco fecha', async () => {
  let viva = true
  const { cliente, chamadas } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a', 'b'])], extensaoEmTodosAplicar: [{ aplicados: ['a', 'b'] }] })
  const sessao = new SessaoEmTodos(cliente, alvo, () => viva)
  sessao.abrir()
  await esperar()
  assert.equal(sessao.estado.fase, 'previa')
  viva = false
  await sessao.confirmar()
  assert.equal(sessao.estado.fase, 'fechado')
  assert.equal(chamadas.filter((c) => c.metodo === 'extensaoEmTodosAplicar').length, 0, 'nenhuma escrita depois da invalidação')
})

test('reconferir: a prévia vira velha quando a origem muda, mas não mexe no que já foi ao Hub', async () => {
  let viva = true
  const { cliente } = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [{ aplicados: ['a'] }] })
  const sessao = new SessaoEmTodos(cliente, alvo, () => viva)
  sessao.abrir()
  await esperar()
  sessao.reconferir()
  assert.equal(sessao.estado.fase, 'previa', 'origem viva: nada muda')
  viva = false
  sessao.reconferir()
  assert.equal(sessao.estado.fase, 'fechado')
  // Aplicando: a resposta ainda é tratada (e descartada pela origem), não é cortada no meio.
  viva = true
  const outra = hubFalso({ extensaoEmTodosPrevia: [previaDoHub('v1', ['a'])], extensaoEmTodosAplicar: [{ aplicados: ['a'] }] })
  const s2 = new SessaoEmTodos(outra.cliente, alvo, () => viva)
  s2.abrir()
  await esperar()
  const pendente = s2.confirmar()
  viva = false
  s2.reconferir()
  assert.equal(s2.estado.fase, 'aplicando')
  await pendente
  assert.equal(s2.estado.fase, 'fechado')
})

test('contrato do #764 (01bae24e): extensão que sumiu entre a prévia e o aplicar vem como conflito; a prévia refeita diz indisponível', async () => {
  const { cliente, chamadas } = hubFalso({
    extensaoEmTodosPrevia: [previaDoHub('v1', ['a']), new ErroHub(409, 'extensoes_em_todos_indisponivel')],
    extensaoEmTodosAplicar: [new ErroHub(409, 'extensoes_em_todos_conflito')],
  })
  const sessao = new SessaoEmTodos(cliente, alvo)
  sessao.abrir()
  await esperar()
  await sessao.confirmar()
  await esperar()
  assert.deepEqual(sessao.estado, { fase: 'erro', erro: 'indisponivel' })
  assert.deepEqual(chamadas.map((c) => c.metodo), ['extensaoEmTodosPrevia', 'extensaoEmTodosAplicar', 'extensaoEmTodosPrevia'])
})

test('interruptor: resposta perdida é reconciliada relendo o Hub; sem leitura, fica incerto (nunca "confirmado")', async () => {
  const lista = (novosBots?: boolean) => ({ skillProtocol: 2, items: [{ id: alvo.id, ...(novosBots === undefined ? {} : { novosBots }) }] })
  // O Hub gravou true, a resposta se perdeu: a releitura diz true.
  const a = hubFalso({ extensaoNovosBotsDefinir: [new ErroHub(0, 'link caiu')], skills: [lista(true)] })
  assert.deepEqual(await alterarNovosBots(a.cliente, alvo, true), { tipo: 'conferido', ligado: true })
  assert.deepEqual(a.chamadas.map((c) => c.metodo), ['extensaoNovosBotsDefinir', 'skills'], 'a gravação nunca é repetida')
  // O Hub não gravou: a releitura diz false (campo ausente = desligado).
  const b = hubFalso({ extensaoNovosBotsDefinir: [new Error('tempo esgotado')], skills: [lista()] })
  assert.deepEqual(await alterarNovosBots(b.cliente, alvo, true), { tipo: 'conferido', ligado: false })
  // Resposta ilegível também é dúvida.
  const c = hubFalso({ extensaoNovosBotsDefinir: [{ id: 'outra', novosBots: true }], skills: [lista(true)] })
  assert.deepEqual(await alterarNovosBots(c.cliente, alvo, true), { tipo: 'conferido', ligado: true })
  // A releitura também falha, ou a skill sumiu da lista: incerto.
  const d = hubFalso({ extensaoNovosBotsDefinir: [new ErroHub(0, 'x')], skills: [new ErroHub(0, 'x')] })
  assert.deepEqual(await alterarNovosBots(d.cliente, alvo, true), { tipo: 'incerto' })
  const e = hubFalso({ extensaoNovosBotsDefinir: [new ErroHub(0, 'x')], skills: [{ skillProtocol: 2, items: [] }] })
  assert.deepEqual(await alterarNovosBots(e.cliente, alvo, true), { tipo: 'incerto' })
  // Erro claro não reconcilia: permissão e Hub antigo.
  const f = hubFalso({ extensaoNovosBotsDefinir: [new ErroHub(403, 'forbidden')] })
  assert.deepEqual(await alterarNovosBots(f.cliente, alvo, true), { tipo: 'erro', erro: 'permissao' })
  assert.equal(f.chamadas.length, 1)
  // Resposta normal.
  const g = hubFalso({ extensaoNovosBotsDefinir: [{ id: alvo.id, novosBots: true }] })
  assert.deepEqual(await alterarNovosBots(g.cliente, alvo, true), { tipo: 'gravado', ligado: true })
  // Conector: recusa sem chamar o Hub, e sem reler.
  const h = hubFalso({})
  assert.deepEqual(await alterarNovosBots(h.cliente, { tipo: 'conector', id: 'x' }, true), { tipo: 'erro', erro: 'generico' })
  assert.equal(h.chamadas.length, 0)
})

test('lerNovosBots lê só o que o Hub diz da skill pedida', async () => {
  const h = hubFalso({ skills: [{ skillProtocol: 2, items: [{ id: 'outra', novosBots: true }, { id: alvo.id, novosBots: true }] }, { skillProtocol: 1, items: [] }] })
  assert.equal(await lerNovosBots(h.cliente, alvo.id), true)
  assert.equal(await lerNovosBots(h.cliente, alvo.id), null, 'protocolo desconhecido não vale')
})

test('interruptor: o valor reconciliado com o Hub não volta ao da lista velha (rodada 2 do #769)', () => {
  // Lista antiga false; a gravação (true) teve a resposta perdida; a releitura do Hub diz true.
  let e = interruptorInicial(false)
  e = proximoInterruptor(e, { tipo: 'resultado', resultado: { tipo: 'conferido', ligado: true }, lista: false })
  assert.deepEqual(e, { ligado: true, incerto: false, conferido: true, aguardaLista: true })
  // A lista velha (false) chegando depois não desliga o interruptor.
  e = proximoInterruptor(e, { tipo: 'lista', valor: false })
  assert.equal(e.ligado, true)
  // A lista nova confirma (true): o valor passa a ser o da lista, sem esperar mais.
  e = proximoInterruptor(e, { tipo: 'lista', valor: true })
  assert.deepEqual(e, { ligado: true, incerto: false, conferido: true, aguardaLista: false })
  // Daí em diante a lista manda de novo (outro aparelho mudou o padrão).
  e = proximoInterruptor(e, { tipo: 'lista', valor: false })
  assert.equal(e.ligado, false)
})

test('interruptor incerto → "Conferir de novo" lê true: fica ligado e a lista velha não o sobrescreve, mesmo se a atualização da página falhar', () => {
  let e = interruptorInicial(false)
  e = proximoInterruptor(e, { tipo: 'resultado', resultado: { tipo: 'incerto' }, lista: false })
  assert.equal(e.incerto, true)
  e = proximoInterruptor(e, { tipo: 'lista', valor: true })
  assert.equal(e.ligado, false, 'em dúvida, a lista não decide')
  e = proximoInterruptor(e, { tipo: 'conferiu', lido: null, lista: false })
  assert.equal(e.incerto, true, 'leitura que falha não resolve a dúvida')
  e = proximoInterruptor(e, { tipo: 'conferiu', lido: true, lista: false })
  assert.deepEqual(e, { ligado: true, incerto: false, conferido: true, aguardaLista: true })
  // Limpar a dúvida não reaplica a lista antiga.
  e = proximoInterruptor(e, { tipo: 'lista', valor: false })
  assert.equal(e.ligado, true)
})

test('interruptor: gravação normal, erro claro e lista que já tem o valor', () => {
  let e = interruptorInicial(false)
  e = proximoInterruptor(e, { tipo: 'resultado', resultado: { tipo: 'gravado', ligado: true }, lista: false })
  assert.deepEqual(e, { ligado: true, incerto: false, conferido: false, aguardaLista: true })
  const antes = e
  assert.equal(proximoInterruptor(e, { tipo: 'resultado', resultado: { tipo: 'erro', erro: 'permissao' }, lista: false }), antes, 'erro não muda o valor')
  // A lista já tinha o valor: nada fica esperando.
  const ja = proximoInterruptor(interruptorInicial(true), { tipo: 'conferiu', lido: true, lista: true })
  assert.equal(ja.aguardaLista, false)
})

test('interruptor: a lista que confirma durante o pedido não deixa o interruptor preso (rodada 3 do #769)', () => {
  // Gravação (resultado): lista inicial false; durante o RPC a lista chega true; a resposta confirma true com a lista ATUAL.
  let e = interruptorInicial(false)
  e = proximoInterruptor(e, { tipo: 'lista', valor: true })
  e = proximoInterruptor(e, { tipo: 'resultado', resultado: { tipo: 'gravado', ligado: true }, lista: true })
  assert.deepEqual(e, { ligado: true, incerto: false, conferido: false, aguardaLista: false })
  // Outro aparelho muda a lista para false: o interruptor acompanha.
  e = proximoInterruptor(e, { tipo: 'lista', valor: false })
  assert.equal(e.ligado, false)

  // O mesmo na reconciliação ("Conferir de novo"): lista true chegou durante a leitura, que também diz true.
  let c = proximoInterruptor(interruptorInicial(false), { tipo: 'resultado', resultado: { tipo: 'incerto' }, lista: false })
  c = proximoInterruptor(c, { tipo: 'lista', valor: true }) // ignorada: em dúvida
  c = proximoInterruptor(c, { tipo: 'conferiu', lido: true, lista: true })
  assert.deepEqual(c, { ligado: true, incerto: false, conferido: true, aguardaLista: false })
  c = proximoInterruptor(c, { tipo: 'lista', valor: false })
  assert.equal(c.ligado, false, 'a lista volta a mandar')
})
