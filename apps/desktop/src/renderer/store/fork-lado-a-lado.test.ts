import { test } from 'node:test'
import assert from 'node:assert/strict'
import { setImmediate as tick } from 'node:timers/promises'
import {
  CHAVE_ARMAZENAMENTO_LADO_A_LADO, MAX_PAINEIS, type LadoALadoEscopo, type LadoALadoEscopoEstado, type LadoALadoRef,
} from '../../shared/fork-lado-a-lado.ts'
import { LadoALadoStore, type LadoALadoAviso } from './fork-lado-a-lado.ts'

// A loja do quadro: persiste o mapa por escopo, devolve o aviso de limite e de remoções,
// sincroniza entre abas e nunca derruba por erro de disco. As regras em si têm teste próprio
// no arquivo do domínio; aqui só o que a loja agrega por cima delas.

/** Armazenamento em memória; `estourar` faz `setItem` falhar como uma quota cheia. */
function memoria(estourar = false) {
  const dados = new Map<string, string>()
  return {
    getItem: (chave: string) => dados.get(chave) ?? null,
    setItem: (chave: string, valor: string) => {
      if (estourar) throw new Error('quota')
      dados.set(chave, valor)
    },
    dump: () => Object.fromEntries(dados),
    gravar: (chave: string, valor: string) => { dados.set(chave, valor) },
  }
}

const ref = (botId: string, extra: Partial<LadoALadoRef> = {}): LadoALadoRef => ({ computerId: 'comp-1', botId, ...extra })
const escopo: LadoALadoEscopo = { computerId: 'comp-1', hostId: 'hub-1', spaceId: 'esp-1' }
const ids = (estado: LadoALadoEscopoEstado) => estado.paineis.map(p => p.id)

function cheia(loja: LadoALadoStore): void {
  loja.abrir(escopo, ref('bot-a', { spaceId: 'esp-1', hubId: 'hub-1' }))
  loja.abrir(escopo, ref('bot-b', { spaceId: 'esp-1', hubId: 'hub-1' }))
  loja.abrir(escopo, ref('bot-c', { spaceId: 'esp-1', hubId: 'hub-1' }))
}

test('loja: escopo inédito devolve o vazio, sempre a mesma referência', () => {
  const loja = new LadoALadoStore(memoria())
  const a = loja.estado(escopo)
  assert.deepEqual(a, { ativo: false, focoId: null, paineis: [] })
  assert.equal(loja.estado(escopo), a)
  assert.equal(loja.estado(null), a)
  assert.equal(loja.aviso, null)
})

test('loja: abrir grava em disco e outra instância lê o mesmo quadro', () => {
  const disco = memoria()
  const loja = new LadoALadoStore(disco)
  const resultado = loja.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' }), { sessionId: 'sess-1' })
  assert.equal(resultado, 'adicionado')
  const estado = loja.estado(escopo)
  assert.equal(estado.ativo, true)
  assert.equal(estado.focoId, estado.paineis[0].id)
  assert.equal(estado.paineis[0].sessionId, 'sess-1')

  const segunda = new LadoALadoStore(disco)
  assert.deepEqual(segunda.estado(escopo), estado)
  // Escopos diferentes são quadros diferentes.
  assert.deepEqual(segunda.estado(null).paineis, [])
})

test('loja: quadro cheio avisa limite e limparAviso apaga', async () => {
  const loja = new LadoALadoStore(memoria())
  cheia(loja)
  let toques = 0
  const off = loja.subscribe(() => toques++)
  const resultado = loja.abrir(escopo, ref('bot-d', { spaceId: 'esp-1' }))
  await tick()
  assert.equal(resultado, 'limite')
  assert.deepEqual(loja.aviso, { kind: 'limite', maximo: MAX_PAINEIS })
  assert.equal(ids(loja.estado(escopo)).length, MAX_PAINEIS)
  loja.limparAviso()
  await tick()
  assert.equal(loja.aviso, null)
  assert.equal(toques, 2)
  off()
})

test('loja: abrir com conversa inicial num quadro vazio abre os dois, inicial primeiro', () => {
  const loja = new LadoALadoStore(memoria())
  const resultado = loja.abrir(escopo, ref('bot-b', { spaceId: 'esp-1' }), {
    inicial: ref('bot-a', { spaceId: 'esp-1' }),
    inicialSessionId: 'sess-a',
    sessionId: 'sess-b',
  })
  assert.equal(resultado, 'adicionado')
  const estado = loja.estado(escopo)
  assert.deepEqual(estado.paineis.map(p => p.ref.botId), ['bot-a', 'bot-b'])
  assert.deepEqual(estado.paineis.map(p => p.sessionId), ['sess-a', 'sess-b'])
  assert.equal(estado.ativo, true)
  assert.equal(estado.focoId, estado.paineis[1].id)

  // Mesma conversa e mesma sessão: o pedido é o próprio inicial, um painel só.
  const loja2 = new LadoALadoStore(memoria())
  const focado = loja2.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' }), {
    inicial: ref('bot-a', { spaceId: 'esp-1' }), inicialSessionId: 'sess-a', sessionId: 'sess-a',
  })
  assert.equal(focado, 'adicionado')
  assert.equal(loja2.estado(escopo).paineis.length, 1)

  // Mesma conversa, sessão diferente: o inicial e o pedido são dois painéis.
  const loja3 = new LadoALadoStore(memoria())
  loja3.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' }), { inicial: ref('bot-a', { spaceId: 'esp-1' }), sessionId: 'sess-b' })
  const duas = loja3.estado(escopo)
  assert.deepEqual(duas.paineis.map(p => p.sessionId), [null, 'sess-b'])

  // Quadro que já tem painéis ignora o inicial.
  const loja4 = new LadoALadoStore(memoria())
  loja4.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' }))
  loja4.abrir(escopo, ref('bot-b', { spaceId: 'esp-1' }), { inicial: ref('bot-z', { spaceId: 'esp-1' }) })
  assert.deepEqual(loja4.estado(escopo).paineis.map(p => p.ref.botId), ['bot-a', 'bot-b'])
})

test('loja: abrir recusado não grava nada nem avisa', async () => {
  const disco = memoria()
  const loja = new LadoALadoStore(disco)
  let toques = 0
  const off = loja.subscribe(() => toques++)
  assert.equal(loja.abrir(escopo, ref('bot-a')), 'escopo_invalido')
  assert.equal(loja.abrir(escopo, { computerId: '', botId: 'bot-a', spaceId: 'esp-1' }), 'escopo_invalido')
  await tick()
  assert.deepEqual(loja.estado(escopo).paineis, [])
  assert.equal(loja.aviso, null)
  assert.deepEqual(disco.dump(), {})
  assert.equal(toques, 0)
  off()
})

test('loja: pedido inválido não abre nem o painel inicial que o acompanha', () => {
  const disco = memoria()
  const loja = new LadoALadoStore(disco)
  const resultado = loja.abrir(escopo, ref('bot-fora'), { inicial: ref('bot-a', { spaceId: 'esp-1', hubId: 'hub-1' }) })
  assert.equal(resultado, 'escopo_invalido')
  // Nada gravado, nada ativado, disco intacto.
  assert.deepEqual(loja.estado(escopo), { ativo: false, focoId: null, paineis: [] })
  assert.deepEqual(disco.dump(), {})
})

test('loja: localStorage que lança no construtor não derruba o quadro', () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, get() { throw new Error('negado') } })
  try {
    const loja = new LadoALadoStore()
    assert.deepEqual(loja.estado(null), { ativo: false, focoId: null, paineis: [] })
    loja.abrir(null, ref('bot-a'))
    assert.equal(loja.estado(null).paineis.length, 1)
  } finally {
    if (original) Object.defineProperty(globalThis, 'localStorage', original)
    else delete (globalThis as Record<string, unknown>).localStorage
  }
})

test('loja: aba defasada não apaga o escopo que a outra gravou', () => {
  const disco = memoria()
  const abaA = new LadoALadoStore(disco)
  const abaB = new LadoALadoStore(disco)
  // Cada aba com seu mapa em memória, sem recarregar entre as gravações.
  abaA.abrir(null, ref('bot-a'))
  abaB.abrir(escopo, ref('bot-b', { spaceId: 'esp-1' }))
  const leitor = new LadoALadoStore(disco)
  assert.equal(leitor.estado(null).paineis.length, 1)
  assert.equal(leitor.estado(escopo).paineis.length, 1)
  // E o mapa defasado da primeira aba também não perde o que a segunda gravou.
  abaA.abrir(null, ref('bot-c'))
  const leitorFinal = new LadoALadoStore(disco)
  assert.equal(leitorFinal.estado(null).paineis.length, 2)
  assert.equal(leitorFinal.estado(escopo).paineis.length, 1)
})

test('loja: JSON corrompido no disco não trava a gravação para sempre', () => {
  const disco = memoria()
  disco.gravar(CHAVE_ARMAZENAMENTO_LADO_A_LADO, '{corrompido')
  const loja = new LadoALadoStore(disco)
  assert.deepEqual(loja.estado(escopo), { ativo: false, focoId: null, paineis: [] })
  assert.equal(loja.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' })), 'adicionado')
  // A gravação substituiu o blob corrompido: uma loja nova recupera o painel do disco.
  const recuperada = new LadoALadoStore(disco)
  assert.equal(recuperada.estado(escopo).paineis.length, 1)
  assert.equal(recuperada.estado(escopo).paineis[0].ref.botId, 'bot-a')
})

test('loja: entrar restaura painéis salvos; sem nenhum, abre o inicial dado', () => {
  const loja = new LadoALadoStore(memoria())
  loja.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' }))
  loja.sair(escopo)
  assert.equal(loja.estado(escopo).ativo, false)

  loja.entrar(escopo, null)
  const restaurado = loja.estado(escopo)
  assert.equal(restaurado.ativo, true)
  assert.deepEqual(restaurado.paineis.map(p => p.ref.botId), ['bot-a'])

  // Sem painéis salvos e sem inicial: nada a abrir.
  const loja2 = new LadoALadoStore(memoria())
  loja2.entrar(escopo, null)
  assert.deepEqual(loja2.estado(escopo).paineis, [])
  assert.equal(loja2.estado(escopo).ativo, false)
  loja2.entrar(escopo, ref('bot-a', { spaceId: 'esp-1' }), 'sess-1')
  const aberto = loja2.estado(escopo)
  assert.equal(aberto.ativo, true)
  assert.deepEqual(aberto.paineis, [{ id: aberto.focoId, ref: ref('bot-a', { spaceId: 'esp-1' }), sessionId: 'sess-1' }])
})

test('loja: alternar entra e sai', () => {
  const loja = new LadoALadoStore(memoria())
  const inicial = ref('bot-a', { spaceId: 'esp-1' })
  loja.alternar(escopo, inicial, 'sess-1')
  assert.equal(loja.estado(escopo).ativo, true)
  loja.alternar(escopo, inicial)
  assert.equal(loja.estado(escopo).ativo, false)
  assert.deepEqual(loja.estado(escopo).paineis.map(p => p.ref.botId), ['bot-a'])
  loja.alternar(escopo, inicial)
  assert.equal(loja.estado(escopo).ativo, true)
})

test('loja: remover, trocar, sessão, mover e focar vão ao estado e ao disco', () => {
  const disco = memoria()
  const loja = new LadoALadoStore(disco)
  cheia(loja)
  const [a, b] = loja.estado(escopo).paineis

  loja.remover(escopo, b.id)
  assert.deepEqual(ids(loja.estado(escopo)), [a.id, loja.estado(escopo).paineis[1].id])

  loja.trocar(escopo, a.id, ref('bot-x', { spaceId: 'esp-1', hubId: 'hub-1' }), 'sess-x')
  const trocado = loja.estado(escopo).paineis.find(p => p.id === a.id)
  assert.deepEqual(trocado?.ref.botId, 'bot-x')
  assert.equal(trocado?.sessionId, 'sess-x')
  assert.equal(loja.estado(escopo).focoId, a.id)

  loja.atualizarSessao(escopo, a.id, null)
  assert.equal(loja.estado(escopo).paineis.find(p => p.id === a.id)?.sessionId, null)

  loja.mover(escopo, a.id, 1)
  assert.equal(ids(loja.estado(escopo))[1], a.id)

  loja.focar(escopo, loja.estado(escopo).paineis[0].id)
  assert.equal(loja.estado(escopo).focoId, loja.estado(escopo).paineis[0].id)

  const segunda = new LadoALadoStore(disco)
  assert.deepEqual(segunda.estado(escopo), loja.estado(escopo))
})

test('loja: ciclarFoco gira o foco e devolve o painel focado', () => {
  const loja = new LadoALadoStore(memoria())
  cheia(loja)
  const antes = loja.estado(escopo).focoId as string
  const primeiro = ids(loja.estado(escopo))[0]
  const foco = loja.ciclarFoco(escopo, 1)
  assert.equal(foco, loja.estado(escopo).focoId)
  assert.notEqual(foco, antes)
  for (let i = 0; i < 3; i++) loja.ciclarFoco(escopo, -1)
  assert.equal(loja.estado(escopo).focoId, primeiro)
  assert.equal(loja.ciclarFoco(null, 1), null)
})

test('loja: reconciliar avisa remoções com o quadro aberto e conta sem avisar fechado', async () => {
  const disco = memoria()
  const loja = new LadoALadoStore(disco)
  cheia(loja)
  const computadores = new Set(['comp-1'])
  const disponiveis = () => loja.estado(escopo).paineis.map(p => ({ ref: { ...p.ref } }))

  const nenhum = loja.reconciliar(escopo, disponiveis(), computadores)
  assert.equal(nenhum, 0)
  assert.equal(loja.aviso, null)

  const comTudo = loja.reconciliar(escopo, [{ ref: ref('bot-a', { spaceId: 'esp-1', hubId: 'hub-1' }) }], computadores)
  assert.equal(comTudo, 2)
  await tick()
  assert.deepEqual(loja.aviso, { kind: 'removidas', count: 2 })
  assert.deepEqual(ids(loja.estado(escopo)).length, 1)
  loja.limparAviso()

  // Quadro fechado: remove na mesma medida, sem aviso.
  loja.sair(escopo)
  const fechado = loja.reconciliar(escopo, [], computadores)
  assert.equal(fechado, 1)
  assert.equal(loja.aviso, null)
  assert.deepEqual(loja.estado(escopo).paineis, [])
})

test('loja: disco que falha não derruba o quadro; disco ilegível começa vazio', async () => {
  const loja = new LadoALadoStore(memoria(true))
  assert.equal(loja.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' })), 'adicionado')
  assert.equal(loja.estado(escopo).paineis.length, 1)
  assert.equal(loja.aviso, null)

  const semDisco = new LadoALadoStore(null)
  assert.equal(semDisco.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' })), 'adicionado')
  assert.equal(semDisco.estado(escopo).paineis.length, 1)

  const lixo = memoria()
  lixo.gravar(CHAVE_ARMAZENAMENTO_LADO_A_LADO, '{isso não é json')
  const leitor = new LadoALadoStore(lixo)
  assert.deepEqual(leitor.estado(escopo).paineis, [])
  assert.equal(leitor.abrir(escopo, ref('bot-a', { spaceId: 'esp-1' })), 'adicionado')

  const lixo2 = memoria()
  lixo2.gravar(CHAVE_ARMAZENAMENTO_LADO_A_LADO, JSON.stringify({ pessoal: { ativo: true, focoId: 'p1', paineis: [{ id: 'p1', ref: { botId: 'b' } }] } }))
  const leitor2 = new LadoALadoStore(lixo2)
  assert.deepEqual(leitor2.estado(null).paineis, [])
  await tick()
})

test('loja: mudança de outra aba chega pelo evento de storage; chave alheia não mexe', () => {
  const global = globalThis as { window?: unknown }
  const original = global.window
  let capturado: unknown
  global.window = { addEventListener: (_tipo: string, handler: unknown) => { capturado = handler } }
  const disparar = (evento: { key: string | null }) => (capturado as (evento: { key: string | null }) => void)(evento)
  try {
    const disco = memoria()
    const loja = new LadoALadoStore(disco)
    assert.equal(typeof capturado, 'function')
    disco.gravar(CHAVE_ARMAZENAMENTO_LADO_A_LADO, JSON.stringify({
      pessoal: { ativo: true, focoId: 'p1', paineis: [{ id: 'p1', ref: { computerId: 'comp-1', botId: 'bot-1' }, sessionId: null }] },
    }))
    disparar({ key: CHAVE_ARMAZENAMENTO_LADO_A_LADO })
    const sincronizado = loja.estado(null)
    assert.equal(sincronizado.ativo, true)
    assert.deepEqual(sincronizado.paineis.map(p => p.id), ['p1'])

    // Chave de outro dado: o quadro não é re-lido.
    disco.gravar(CHAVE_ARMAZENAMENTO_LADO_A_LADO, JSON.stringify({ pessoal: { ativo: false, focoId: null, paineis: [] } }))
    disparar({ key: 'outro.dado' })
    assert.equal(loja.estado(null).ativo, true)
  } finally {
    if (original === undefined) delete global.window
    else global.window = original
  }
})

test('loja: uma mudança, um aviso de cada vez', async () => {
  const loja = new LadoALadoStore(memoria())
  const avisos: (LadoALadoAviso | null)[] = []
  const off = loja.subscribe(() => avisos.push(loja.aviso))
  cheia(loja)
  loja.abrir(escopo, ref('bot-d', { spaceId: 'esp-1' }))
  await tick()
  assert.deepEqual(avisos.at(-1), { kind: 'limite', maximo: MAX_PAINEIS })
  loja.limparAviso()
  await tick()
  assert.equal(avisos.filter(a => a === null).length, 1)
  off()
})
