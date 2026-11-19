// "Compactando…" no chat (opção A, aprovada pelo dono): o envio espera, o rascunho fica e o envio sempre volta.
import assert from 'node:assert/strict'
import test from 'node:test'
import { ComposerDrafts } from '../../../../shared/composer-drafts.ts'
import { bloqueiaEnvioCompactando, compactacaoDaConversa, medidaDaMarca, ModeloContexto, type Compactacao } from './estado.ts'
import { traduzir, CATALOGOS } from '../../../lib/fork-i18n/index.ts'

const ctx = (over: Record<string, unknown> = {}) => ({ used: 60_000, size: 200_000, updatedAt: 1, trigger: 180_000, canCompact: true, busy: false, contextId: 'c1', ...over })
const resp = (over: Record<string, unknown> = {}) => ({ hubId: 'h', botId: 'a', sessionId: 'a', scopeId: 'A', epoch: 0, context: ctx(), ...over })

/** Relógio manual: o limite de espera avança só quando o teste manda. */
function relogioManual() {
  let agora = 0
  const tarefas = new Map<number, { f: () => void; em: number }>()
  let id = 0
  return {
    relogio: {
      agora: () => agora,
      agendar: ((f: () => void, ms: number) => { tarefas.set(++id, { f, em: agora + ms }); return id }) as unknown as typeof setTimeout,
      cancelar: ((n: number) => tarefas.delete(n)) as unknown as typeof clearTimeout,
    },
    passar: (ms: number) => { agora += ms; for (const [n, t] of [...tarefas]) if (t.em <= agora) { tarefas.delete(n); t.f() } },
    pendentes: () => tarefas.size,
  }
}
const proximoTick = () => new Promise<void>((r) => queueMicrotask(r))
const rascunhos = () => new ComposerDrafts({ getItem: () => null, setItem: () => {} }, 'k')
const modelo = (client: never = { call: async () => resp() } as never, limite = 90_000, relogio = relogioManual()) =>
  ({ m: new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }), limite, relogio.relogio), relogio })

test('compactando bloqueia o envio; terminar, falha e limite de espera liberam', () => {
  const casos: [string, Parameters<typeof compactacaoDaConversa>[0], Compactacao['fase'] | null, boolean][] = [
    ['atividade resumindo', { atividade: 'ativa', emAndamento: false, resultado: null, recusa: null }, 'andamento', true],
    ['Comprimir agora aceito e sem desfecho', { atividade: null, emAndamento: true, resultado: { fase: 'andamento', antes: 1, depois: null }, recusa: null }, 'andamento', true],
    ['terminou, sem marca', { atividade: null, emAndamento: false, resultado: null, recusa: null }, null, false],
    ['terminou bem, com medida', { atividade: null, emAndamento: false, resultado: { fase: 'feito', antes: 60_000, depois: 22_000 }, recusa: null }, 'feito', false],
    ['terminou bem, sem medida', { atividade: null, emAndamento: false, resultado: { fase: 'feito', antes: 60_000, depois: null }, recusa: null }, 'feitoSemMedida', false],
    ['falha do pedido', { atividade: null, emAndamento: false, resultado: null, recusa: 'falha' }, 'falha', false],
    ['não deu para confirmar (90 s)', { atividade: null, emAndamento: false, resultado: { fase: 'incerto', antes: 1, depois: null }, recusa: null }, 'incerto', false],
    ['atividade passou do limite', { atividade: 'expirou', emAndamento: false, resultado: null, recusa: null }, 'incerto', false],
    ['recusa que não é falha (ocupada) não vira marca', { atividade: null, emAndamento: false, resultado: null, recusa: 'ocupada' }, null, false],
  ]
  for (const [nome, entrada, fase, bloqueia] of casos) {
    const c = compactacaoDaConversa(entrada)
    assert.equal(c?.fase ?? null, fase, nome)
    assert.equal(bloqueiaEnvioCompactando(c), bloqueia, nome)
  }
})

test('compactando → rascunho preservado → envio liberado ao terminar', async () => {
  const { m } = modelo()
  const d = rascunhos()
  d.edit('a', 'E o aditivo de prazo, quem precisa assinar?', 'typed')
  const soltar = m.observarAtividade('a', null)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, 'compactando: envio desligado')
  // Tentar enviar não manda nem apaga: o rascunho segue como estava.
  assert.equal(d.get('a'), 'E o aditivo de prazo, quem precisa assinar?')
  // A atividade repetida a cada render não reinicia o relógio nem avisa de novo.
  await proximoTick()
  let avisos = 0
  m.subscribe(() => avisos++)
  const outra = m.observarAtividade('a', null) // o fluxo observa a mesma atividade que o campo
  await proximoTick()
  assert.equal(avisos, 0, 'o segundo observador divide o registro')
  outra()
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, 'um observador ainda vale')
  soltar()
  await proximoTick()
  assert.equal(avisos, 1)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, 'terminou: envio volta')
  assert.equal(d.get('a'), 'E o aditivo de prazo, quem precisa assinar?', 'o rascunho preservado pode ser enviado agora')
})

test('uma conversa compactando não bloqueia outra, nem a sessão extra', () => {
  const { m } = modelo()
  m.observarAtividade('a', null)
  assert.equal(m.compactacao('b', null), null)
  assert.equal(m.compactacao('a', 's2'), null)
})

test('atividade sem fim: depois do limite o envio volta e a marca vira "não deu para confirmar"', async () => {
  const { m, relogio } = modelo()
  const soltar = m.observarAtividade('a', null)
  let avisos = 0
  m.subscribe(() => avisos++)
  relogio.passar(89_000)
  assert.equal(m.compactacao('a', null)?.fase, 'andamento')
  relogio.passar(1_000)
  await proximoTick()
  assert.equal(avisos, 1, 'a tela é avisada sem depender de leitura')
  assert.equal(m.compactacao('a', null)?.fase, 'incerto')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
  // Outra tela passa a observar a mesma atividade: não rearma o bloqueio.
  const tarde = m.observarAtividade('a', null)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
  tarde()
  soltar()
  assert.equal(m.compactacao('a', null), null)
  assert.equal(relogio.pendentes(), 0, 'sem relógio sobrando')
})

test('desmontar a tela solta o envio e o relógio', () => {
  const { m, relogio } = modelo()
  const soltar = m.observarAtividade('a', null)
  soltar()
  soltar() // soltar duas vezes não desfaz o registro de outra tela
  assert.equal(m.compactacao('a', null), null)
  assert.equal(relogio.pendentes(), 0)
})

test('o limite de 90 s vale mesmo depois de trocar de conexão: o registro e o relógio sobrevivem a descartar', () => {
  const { m, relogio } = modelo()
  m.observarAtividade('a', null)
  relogio.passar(60_000)
  m.descartar('a', null) // BarraContexto descarta a conversa quando a conexão ou o Hub muda
  assert.equal(m.compactacao('a', null)?.fase, 'andamento', 'segue bloqueado, sem registro perdido')
  assert.equal(relogio.pendentes(), 1, 'o relógio continua armado')
  relogio.passar(30_000)
  assert.equal(m.compactacao('a', null)?.fase, 'incerto', '90 s contados do começo, não da troca')
})

test('o limite conta do primeiro sinal: atividade que chega perto do fim do pedido manual não o estende', async () => {
  let leitura: ReturnType<typeof resp> = resp()
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? leitura : { accepted: true }) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  relogio.passar(85_000)
  m.observarAtividade('a', null) // o Hub só mostra a atividade perto do fim
  assert.equal(m.compactacao('a', null)?.fase, 'andamento')
  relogio.passar(5_000)
  assert.equal(m.compactacao('a', null)?.fase, 'incerto', 'venceu aos 90 s do pedido, não aos 175 s')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
})

test('a tela decide: com a atividade encerrada, um registro ainda não solto não prende o envio', () => {
  const { m } = modelo()
  m.observarAtividade('a', null)
  assert.equal(m.compactacao('a', null, false), null)
})

test('pedido em voo que nunca responde também libera aos 90 s', async () => {
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? resp() : new Promise(() => {})) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  void m.comprimir('a', null, 'c1')
  await proximoTick()
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true)
  relogio.passar(90_000)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
})

test('falha do pedido de compactar libera o envio e mostra o erro leigo', async () => {
  const client = { call: async (metodo: string) => { if (metodo === 'donnaContext') return resp(); throw new Error('boom') } } as never
  const { m } = modelo(client)
  await m.ler('a', null)
  const p = m.comprimir('a', null, 'c1')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, 'durante o pedido')
  await p
  const c = m.compactacao('a', null)
  assert.equal(c?.fase, 'falha')
  assert.equal(bloqueiaEnvioCompactando(c), false, 'a falha também libera o envio')
  const texto = [traduzir('pt-BR', 'contexto.compactando.falha.titulo'), traduzir('pt-BR', 'contexto.compactando.liberado')].join(' ')
  assert.equal(texto, 'Não deu para compactar agora Você já pode enviar mensagens.')
  assert.doesNotMatch(texto, /erro|token|cache|scope|boom|context_/i, 'sem jargão nem código')
})

test('pedido aceito: andamento bloqueia; "feito" libera com a medida; "não deu para confirmar" libera', async () => {
  let leitura: ReturnType<typeof resp> = resp()
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? leitura : { accepted: true }) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true)
  leitura = resp({ epoch: 1, context: ctx({ used: 22_000, lastCompaction: { at: 9 } }) })
  await m.ler('a', null)
  const c = m.compactacao('a', null)
  assert.deepEqual(c, { fase: 'feito', antes: 60_000, depois: 22_000 })
  assert.equal(bloqueiaEnvioCompactando(c), false)
  const medida = medidaDaMarca('pt-BR', c!)
  assert.deepEqual([medida?.antes, medida?.depois].map((x) => x?.replace(/[\u00a0\u202f]/g, ' ')), ['60 mil', '22 mil'])
  // Outro pedido que nunca termina: o limite de 90 s libera.
  leitura = resp({ epoch: 1, context: ctx({ used: 22_000, busy: true, lastCompaction: { at: 9 } }) })
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.compactacao('a', null)?.fase, 'andamento')
  relogio.passar(90_000)
  assert.equal(m.compactacao('a', null)?.fase, 'incerto')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
})

test('a marca sem medida nunca inventa número', () => {
  assert.equal(medidaDaMarca('pt-BR', { fase: 'feitoSemMedida' }), null)
  assert.equal(medidaDaMarca('pt-BR', { fase: 'andamento' }), null)
})

test('textos do Compactando… existem nos seis idiomas', () => {
  const chaves = ['titulo', 'detalhe', 'concluida', 'medida', 'incerto.titulo', 'falha.titulo', 'liberado', 'motivo', 'tentou'].map((k) => `contexto.compactando.${k}`)
  assert.equal(Object.keys(CATALOGOS).length, 6)
  for (const idioma of Object.keys(CATALOGOS)) for (const k of chaves) {
    const v = (CATALOGOS as Record<string, Record<string, string>>)[idioma]![k]
    assert.ok(v && v.length > 0, `${idioma} ${k}`)
  }
  assert.equal(traduzir('pt-BR', 'contexto.compactando.titulo'), 'Compactando…')
  assert.equal(traduzir('pt-BR', 'contexto.compactando.motivo'), 'Compactando a conversa. Seu texto fica guardado e o envio volta sozinho quando terminar.')
})

test('pedido sem resposta + atividade tardia: o prazo é absoluto a partir do envio (não vai a 175 s)', async () => {
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? resp() : new Promise(() => {})) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  void m.comprimir('a', null, 'c1')
  await proximoTick()
  relogio.passar(85_000)
  m.observarAtividade('a', null) // o Hub só mostra a atividade perto do fim
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, '85 s: ainda esperando')
  relogio.passar(5_000)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, '90 s do envio: libera')
  assert.equal(m.compactacao('a', null)?.fase, 'incerto')
})

test('aceite tardio não reinicia o prazo: o pedido aceito aos 89 s vence aos 90 s do envio, não aos 179 s', async () => {
  let aceitar!: (v: unknown) => void
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? resp() : new Promise((ok) => { aceitar = ok })) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  const p = m.comprimir('a', null, 'c1')
  await proximoTick()
  relogio.passar(89_000)
  aceitar({ accepted: true })
  await p
  assert.equal(m.compactacao('a', null)?.fase, 'andamento', 'aos 89 s ainda espera')
  relogio.passar(1_000)
  assert.equal(m.compactacao('a', null)?.fase, 'incerto', 'venceu aos 90 s contados do envio')
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
})

test('sequência da revisão: t=0 pedido, t=85 atividade, troca de conexão, t=90 libera (o início do primeiro sinal sobrevive)', async () => {
  for (const aceito of [false, true]) {
    let aceitar!: (v: unknown) => void
    const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? resp() : aceito ? { accepted: true } : new Promise((ok) => { aceitar = ok })) } as never
    const { m, relogio } = modelo(client)
    await m.ler('a', null)
    void m.comprimir('a', null, 'c1') // t = 0
    await proximoTick()
    relogio.passar(85_000)
    m.observarAtividade('a', null) // t = 85
    m.descartar('a', null) // troca de conexão: BarraContexto descarta o pedido
    assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, `85 s (${aceito ? 'aceito' : 'em voo'}): ainda esperando`)
    relogio.passar(4_999)
    assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, '89,999 s')
    relogio.passar(1)
    assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, `90 s (${aceito ? 'aceito' : 'em voo'}): libera`)
    relogio.passar(85_000) // e não volta a bloquear até 175 s
    assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, '175 s')
    void aceitar
  }
})

test('o início do primeiro sinal sobrevive à troca de conexão ANTES da atividade e à desmontagem/remontagem da tela', () => {
  const { m, relogio } = modelo()
  // pedido descartado aos 60 s e atividade só aos 85 s: é a mesma compactação
  const a = modelo()
  a.m.observarAtividade('a', null)
  a.relogio.passar(60_000)
  a.m.descartar('a', null)
  a.relogio.passar(25_000)
  a.m.observarAtividade('a', null) // outra tela
  a.relogio.passar(5_000)
  assert.equal(bloqueiaEnvioCompactando(a.m.compactacao('a', null)), false, '90 s do primeiro sinal')
  // tela desmontada e remontada com a compactação ainda em curso: o prazo continua
  const soltar = m.observarAtividade('a', null)
  relogio.passar(60_000)
  soltar()
  const soltar2 = m.observarAtividade('a', null)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true)
  relogio.passar(30_000)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, 'remontar não reinicia')
  soltar2()
})

test('compactação que acaba de verdade apaga o início; uma nova depois conta de novo, e um sinal novo após o vencimento também', async () => {
  const { m, relogio } = modelo()
  const soltar = m.observarAtividade('a', null)
  relogio.passar(50_000)
  soltar()
  m.atividadeAcabou('a', null) // o bot deixou de resumir
  relogio.passar(10_000)
  m.observarAtividade('a', null) // outra compactação, aos 60 s
  relogio.passar(80_000)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true, 'conta de novo: 80 s depois ainda espera')
  relogio.passar(10_000)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false)
  // sinal antigo vencido, sem sinal em curso: um sinal novo recomeça
  const { m: n, relogio: r2 } = modelo()
  const s1 = n.observarAtividade('a', null)
  r2.passar(95_000)
  s1()
  n.observarAtividade('a', null)
  assert.equal(bloqueiaEnvioCompactando(n.compactacao('a', null)), true, 'início novo após vencer e sem sinais')
})

test('pedido manual durante uma atividade em curso não reinicia o prazo (continua contando do primeiro sinal)', async () => {
  const client = { call: async (metodo: string) => (metodo === 'donnaContext' ? resp() : new Promise(() => {})) } as never
  const { m, relogio } = modelo(client)
  await m.ler('a', null)
  m.observarAtividade('a', null) // t = 0
  relogio.passar(50_000)
  void m.comprimir('a', null, 'c1') // t = 50: outra janela, atalho…
  await proximoTick()
  relogio.passar(39_999)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), true)
  relogio.passar(1)
  assert.equal(bloqueiaEnvioCompactando(m.compactacao('a', null)), false, '90 s do primeiro sinal, não 140 s')
})
