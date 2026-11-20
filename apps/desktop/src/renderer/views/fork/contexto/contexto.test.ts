import assert from 'node:assert/strict'
import test from 'node:test'
import { ROUTING_PERSONAL } from '../../../../shared/fork-roteamento.ts'
import { avisoSemSuporte, cheio, codigoRecusa, conferirContexto, hora, ModeloContexto, motivoBloqueio, percentual, porcento, resumidaEm, tokensCurtos, vistaBalao, VALIDADE_MS, type EntradaVista, type Origem } from './estado.ts'
import { CATALOGOS, traduzir } from '../../../lib/fork-i18n/index.ts'

const ctx = (over: Record<string, unknown> = {}) => ({ used: 84_000, size: 200_000, updatedAt: 1, trigger: 180_000, canCompact: true, busy: false, contextId: 'c1', ...over })
/** O Intl separa número e unidade com espaço não separável (U+00A0/U+202F); a comparação usa espaço comum. */
const sp = (x: string) => x.replace(/[\u00a0\u202f]/g, ' ')
const tick = () => new Promise((r) => setTimeout(r, 0))
/** A resposta de `donnaContext` no formato do contrato: principal volta com `sessionId` = bot. */
const resp = (botId: string, over: Record<string, unknown> = {}, sessionId: string | null = null) => ({ hubId: 'h', botId, sessionId: sessionId ?? botId, scopeId: 'A', epoch: 0, context: ctx(), ...over })
/** Forma da resposta de `donnaContext` (hubId, botId, sessionId, scopeId, epoch, context). */
type RespostaHub = ReturnType<typeof resp>
const pedido = (botId: string, sessionId: string | null = null) => ({ hubId: 'h', botId, sessionId })

test('context ausente ou sem tamanho: resposta válida, medidor oculto', () => {
  assert.deepEqual(conferirContexto(resp('a', { context: undefined }), pedido('a')), { scopeId: 'A', epoch: 0, leitura: null })
  assert.equal(conferirContexto(resp('a', { context: ctx({ size: 0 }) }), pedido('a'))?.leitura, null)
  assert.equal(conferirContexto(resp('a', { context: ctx({ contextId: '' }) }), pedido('a'))?.leitura, null)
  const l = conferirContexto(resp('a', { context: ctx({ lastCompaction: { at: 5, byHost: true } }) }), pedido('a'))?.leitura
  assert.deepEqual(l?.lastCompaction, { at: 5, byHost: true })
})

test('#2 resposta só vale para o mesmo Hub, conversa, sessão exata e epoch válido', () => {
  assert.equal(conferirContexto(resp('a'), pedido('a'))?.leitura?.used, 84_000)
  assert.equal(conferirContexto(resp('a', { hubId: 'outro' }), pedido('a')), null, 'outro Hub')
  assert.equal(conferirContexto(resp('b'), pedido('a')), null, 'outra conversa')
  assert.equal(conferirContexto(resp('a', {}, 's2'), pedido('a')), null, 'extra respondendo pela principal')
  assert.equal(conferirContexto(resp('a'), pedido('a', 's2')), null, 'principal respondendo pela extra')
  assert.equal(conferirContexto(resp('a', {}, 's3'), pedido('a', 's2')), null, 'outra extra')
  assert.equal(conferirContexto(resp('a', {}, 's2'), pedido('a', 's2'))?.leitura?.used, 84_000)
  for (const epoch of [undefined, -1, 1.5, '3', Number.NaN]) assert.equal(conferirContexto(resp('a', { epoch }), pedido('a')), null, `epoch ${epoch}`)
  for (const scopeId of [undefined, '', 3]) assert.equal(conferirContexto(resp('a', { scopeId }), pedido('a')), null, `scopeId ${scopeId}`)
})

test('resposta com spaceId vale só no Space do pedido; Hub antigo (sem spaceId) só no Pessoal', () => {
  const pessoal = ROUTING_PERSONAL
  const p = (spaceId: string) => ({ ...pedido('a'), spaceId })
  assert.equal(conferirContexto(resp('a', { spaceId: 's-a' }), p('s-a'))?.leitura?.used, 84_000, 'mesmo Space')
  assert.equal(conferirContexto(resp('a', { spaceId: 's-b' }), p('s-a')), null, 'outro Space')
  assert.equal(conferirContexto(resp('a'), p('s-a')), null, 'sem spaceId não prova o Space')
  assert.equal(conferirContexto(resp('a', { spaceId: pessoal }), pedido('a'))?.leitura?.used, 84_000, 'Pessoal explícito')
  assert.equal(conferirContexto(resp('a'), pedido('a'))?.leitura?.used, 84_000, 'sem spaceId = Pessoal legado')
  assert.equal(conferirContexto(resp('a', { spaceId: 's-a' }), pedido('a')), null, 'pedido sem Space vale só Pessoal')
})

test('percentual, vermelho acima de 90% e mil tokens', () => {
  assert.equal(percentual({ used: 84_000, size: 200_000 }), 42)
  assert.equal(percentual({ used: 300, size: 200 }), 100)
  assert.equal(cheio(90), false)
  assert.equal(cheio(91), true)
  assert.equal(sp(tokensCurtos('pt-BR', 84_000)), '84 mil')
  assert.equal(sp(tokensCurtos('pt-BR', 1_200_000)), '1,2 mi')
  assert.equal(sp(tokensCurtos('pt-BR', 512)), '512')
  assert.equal(sp(tokensCurtos('en', 84_000)), '84K')
  assert.equal(sp(tokensCurtos('en', 1_200_000)), '1.2M')
  assert.equal(porcento('pt-BR', 42), '42%')
  assert.equal(porcento('en', 42), '42%')
})

test('Comprimir agora desligado em turno, busy, sem conexão e enviando', () => {
  const base = { busy: false, online: true, working: false, enviando: false }
  assert.equal(motivoBloqueio(base), null)
  assert.equal(motivoBloqueio({ ...base, busy: true }), 'turno')
  assert.equal(motivoBloqueio({ ...base, working: true }), 'turno')
  assert.equal(motivoBloqueio({ ...base, online: false }), 'semConexao')
  assert.equal(motivoBloqueio({ ...base, enviando: true }), 'enviando')
})

test('resposta de outra conexão é descartada e não aparece', async () => {
  let resolver!: (v: unknown) => void
  const velho = { call: () => new Promise((r) => { resolver = r }) } as never
  let origem: Origem | null = { client: velho, hostId: 'h', epoch: 1 }
  const m = new ModeloContexto(() => origem)
  const pedido = m.ler('a', null)
  origem = { client: { call: async () => ({ ...resp('a', { context: ctx({ used: 1000 }) }), hubId: 'h2' }) } as never, hostId: 'h2', epoch: 2 }
  resolver(resp('a'))
  await pedido
  assert.equal(m.of('a', null), null)
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 1000)
  origem = { ...origem, epoch: 3 }
  assert.equal(m.of('a', null), null, 'reconexão: leitura antiga não vale')
})

test('Hub sem donnaContext: para de pedir até outra conexão', async () => {
  let chamadas = 0
  const client = { call: async () => { chamadas++; throw Object.assign(new Error('unknown method'), { status: 404 }) } } as never
  let origem: Origem = { client, hostId: 'h', epoch: 1 }
  const m = new ModeloContexto(() => origem)
  await m.ler('a', null)
  assert.equal(m.suportado(), false)
  await m.ler('a', null)
  assert.equal(chamadas, 1)
  origem = { ...origem, epoch: 2 }
  assert.equal(m.suportado(), true)
})

test('comprimir envia o contextId da leitura e a sessão só quando não é a principal', async () => {
  const corpos: unknown[] = []
  const client = { call: async (method: string, body: unknown) => { corpos.push([method, body]); return method === 'donnaContext' ? resp('a', {}, (body as { sessionId?: string }).sessionId ?? null) : { accepted: true } } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null); await m.ler('a', 's2'); corpos.length = 0
  await m.comprimir('a', null, 'c1')
  await m.comprimir('a', 's2', 'c1')
  await tick()
  assert.deepEqual(corpos, [
    ['donnaContextCompact', { botId: 'a', contextId: 'c1' }], ['donnaContext', { botId: 'a' }],
    ['donnaContextCompact', { botId: 'a', sessionId: 's2', contextId: 'c1' }], ['donnaContext', { botId: 'a', sessionId: 's2' }],
  ])
})

test('C2-R1-02: recusa vira código estável, sem texto do Hub; a view traduz', () => {
  assert.equal(codigoRecusa(new Error('context_repeat_guard')), 'repetida')
  assert.equal(codigoRecusa({ message: 'context_busy' }), 'ocupada')
  assert.equal(codigoRecusa(new Error('context_engine_changed')), 'mudou')
  assert.equal(codigoRecusa(new Error('context_unavailable')), 'indisponivel')
  assert.equal(codigoRecusa(new Error('context_engine_unavailable')), 'indisponivel', 'motor desligado agora: passageiro, não "sem suporte"')
  assert.equal(codigoRecusa(new Error('unknown_session')), 'mudou')
  assert.equal(codigoRecusa(new Error('context_cannot_compact')), 'semMotor')
  assert.equal(codigoRecusa(new Error('unknown method donnaContextCompact')), 'antigo')
  assert.equal(codigoRecusa(new Error('timeout')), 'falha')
  assert.equal(traduzir('pt-BR', 'contexto.aviso.repetida.titulo'), 'Já foi resumida há pouco')
  assert.equal(traduzir('en', 'contexto.aviso.falha.titulo'), "Couldn't compact right now")
})

test('C2-R1-02: recusa fica na conversa e conexão dela, sobrevive à releitura e some no novo pedido', async () => {
  let recusar = true
  const client = { call: async (method: string, body: { botId: string }) => {
    if (method === 'donnaContext') return resp(body.botId)
    if (recusar) throw new Error('context_repeat_guard')
    return { accepted: true }
  } } as never
  let origem: Origem = { client, hostId: 'h', epoch: 1 }
  const m = new ModeloContexto(() => origem)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.recusa('a', null), 'repetida')
  assert.equal(m.recusa('a', 's2'), null, 'outra sessão')
  assert.equal(m.recusa('b', null), null, 'outra conversa')
  await m.ler('a', null)
  assert.equal(m.recusa('a', null), 'repetida', 'releitura da mesma origem mantém')
  origem = { ...origem, epoch: 2 }
  assert.equal(m.recusa('a', null), null, 'outra conexão')
  origem = { ...origem, epoch: 1 }
  recusar = false
  await m.comprimir('a', null, 'c1')
  assert.equal(m.recusa('a', null), null, 'sucesso limpa')
})

test('Comprimir agora: aceito → andamento → feito com tokens antes/depois (nunca silencioso)', async () => {
  let lc: { at: number; byHost: boolean } | undefined
  let used = 60_000
  let busy = false
  const client = { call: async (method: string, body: { botId: string }) =>
    method === 'donnaContext' ? resp(body.botId, { context: ctx({ used, busy, lastCompaction: lc }) }) : (busy = true, { accepted: true }) } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.resultado('a', null)?.fase, 'andamento')
  busy = false; used = 20_000; lc = { at: 99, byHost: true }
  await m.ler('a', null)
  assert.deepEqual(m.resultado('a', null), { fase: 'feito', antes: 60_000, depois: 20_000 })
  assert.equal(sp(traduzir('pt-BR', 'contexto.aviso.feito.titulo', { antes: tokensCurtos('pt-BR', 60_000), depois: tokensCurtos('pt-BR', 20_000) })), 'Pronto: de 60 mil para 20 mil')
  assert.equal(m.resultado('a', 's2'), null, 'outra sessão')
})

test('Comprimir agora: aceito mas o Hub termina sem comprimir → sem efeito, dito ao usuário', async () => {
  let busy = false
  const client = { call: async (method: string, body: { botId: string }) =>
    method === 'donnaContext' ? resp(body.botId, { context: ctx({ busy }) }) : (busy = true, { accepted: true }) } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.resultado('a', null)?.fase, 'andamento')
  busy = false
  await m.ler('a', null)
  assert.equal(m.resultado('a', null)?.fase, 'semEfeito')
})

test('Comprimir agora: Hub antigo e motor sem suporte viram recusa clara; sem motor, botão desligado com motivo', async () => {
  const antigo = { call: async (method: string, body: { botId: string }) => { if (method === 'donnaContext') return resp(body.botId); throw Object.assign(new Error('unknown method'), { status: 404 }) } } as never
  const m = new ModeloContexto(() => ({ client: antigo, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.recusa('a', null), 'antigo')
  assert.equal(motivoBloqueio({ busy: false, online: true, working: false, enviando: false, canCompact: false }), 'semMotor')
  for (const l of Object.keys(CATALOGOS)) for (const k of ['contexto.aviso.semMotor.titulo', 'contexto.aviso.antigo.titulo', 'contexto.aviso.andamento.titulo', 'contexto.aviso.feito.titulo', 'contexto.aviso.feitoSemMedida.titulo', 'contexto.aviso.semEfeito.titulo', 'contexto.aviso.incerto.titulo', 'contexto.aviso.mudou.titulo'])
    assert.ok((CATALOGOS as Record<string, Record<string, string>>)[l][k], `${l} ${k}`)
})

test('#3 extra sem medida fica oculta mesmo com a principal medida', async () => {
  const client = { call: async (_m: string, body: { botId: string; sessionId?: string }) =>
    body.sessionId === undefined ? resp(body.botId) : resp(body.botId, { context: undefined }, body.sessionId) } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.ler('a', 's2')
  assert.equal(m.of('a', null)?.used, 84_000)
  assert.equal(m.of('a', 's2'), null)
})

test('#3 sessão com o id do bot é uma extra, nunca a principal', async () => {
  const corpos: unknown[] = []
  const client = { call: async (_m: string, body: { botId: string; sessionId?: string }) => { corpos.push(body); return resp(body.botId, {}, body.sessionId ?? null) } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', 'a')
  assert.deepEqual(corpos, [{ botId: 'a', sessionId: 'a' }])
  assert.equal(m.of('a', null), null, 'leitura pedida como extra não aparece na principal')
})

test('epoch de compactação mais velho que o já visto na mesma conexão é descartado', async () => {
  let proxima: RespostaHub = resp('a', { epoch: 2, context: ctx({ used: 31_000 }) })
  const client = { call: async () => proxima } as never
  let origem: Origem = { client, hostId: 'h', epoch: 1 }
  const m = new ModeloContexto(() => origem)
  await m.ler('a', null)
  proxima = resp('a', { epoch: 1, context: ctx({ used: 150_000 }) })
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 31_000, 'observação antiga não volta')
  proxima = resp('a', { epoch: 2, context: ctx({ used: 40_000 }) })
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 40_000, 'mesma época atualiza')
  origem = { ...origem, epoch: 2 }
  proxima = resp('a', { epoch: 0, context: ctx({ used: 5000 }) })
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 5000, 'outra conexão recomeça a contagem')
})

test('resposta inválida esconde a medida anterior, sem fallback', async () => {
  let proxima: RespostaHub = resp('a')
  const m = new ModeloContexto(() => ({ client: { call: async () => proxima } as never, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  proxima = resp('a', { hubId: 'outro' })
  await m.ler('a', null)
  assert.equal(m.of('a', null), null)
})

test('C2-R2-01: nova sessão no mesmo lugar (outro scopeId) recomeça a época sem fallback', async () => {
  const pendentes: ((v: unknown) => void)[] = []
  let proxima: unknown = resp('a', { scopeId: 'A', epoch: 3, context: ctx({ used: 120_000 }) })
  let segurar = false
  const client = { call: () => segurar ? new Promise((r) => pendentes.push(r)) : Promise.resolve(proxima) } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 120_000)
  // Pedido ao escopo A que vai atrasar.
  segurar = true
  const atrasado = m.ler('a', null)
  segurar = false
  // Rollover: escopo B, época 0, ainda sem medida → oculto (nunca a medida de A).
  proxima = resp('a', { scopeId: 'B', epoch: 0, context: undefined })
  await m.ler('a', null)
  assert.equal(m.of('a', null), null)
  // B medido na época 0: aceito, apesar de A ter chegado a 3.
  proxima = resp('a', { scopeId: 'B', epoch: 0, context: ctx({ used: 9000 }) })
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 9000)
  // A resposta velha de A chega agora: descartada pela sequência.
  pendentes[0]!(resp('a', { scopeId: 'A', epoch: 3, context: ctx({ used: 120_000 }) }))
  await atrasado
  assert.equal(m.of('a', null)?.used, 9000)
  // No mesmo escopo B a época continua monotônica.
  proxima = resp('a', { scopeId: 'B', epoch: 2, context: ctx({ used: 20_000 }) })
  await m.ler('a', null)
  proxima = resp('a', { scopeId: 'B', epoch: 1, context: ctx({ used: 99_000 }) })
  await m.ler('a', null)
  assert.equal(m.of('a', null)?.used, 20_000)
})

test('UI01: principal medida → extra sem medida oculta → principal continua medida, sem fallback', async () => {
  const pedidos: unknown[] = []
  const client = { call: async (_m: string, body: { botId: string; sessionId?: string }) => {
    pedidos.push(body)
    return body.sessionId === undefined ? resp(body.botId, { scopeId: 'principal' }) : resp(body.botId, { scopeId: 'extra', context: undefined }, body.sessionId)
  } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('byte', null)
  assert.equal(m.of('byte', null)?.used, 84_000)
  // A aba extra pede só a própria sessão e não herda a medida da principal.
  assert.equal(m.of('byte', 'byte-guard'), null, 'antes da resposta')
  await m.ler('byte', 'byte-guard')
  assert.equal(m.of('byte', 'byte-guard'), null, 'extra sem context')
  // De volta à principal: a medida dela continua, intacta.
  assert.equal(m.of('byte', null)?.used, 84_000)
  await m.ler('byte', null)
  assert.equal(m.of('byte', null)?.used, 84_000)
  assert.deepEqual(pedidos, [{ botId: 'byte' }, { botId: 'byte', sessionId: 'byte-guard' }, { botId: 'byte' }])
})

test('DON-903: todo contexto.* existe nos seis catálogos, sem vazio, e o texto segue o idioma', () => {
  const chaves = Object.keys(CATALOGOS['pt-BR']).filter((k) => k.startsWith('contexto.') && !k.startsWith('contexto.custo.'))
  assert.equal(chaves.length, 111)
  for (const [idioma, cat] of Object.entries(CATALOGOS)) for (const k of chaves) assert.ok((cat as Record<string, string>)[k]?.trim(), `${idioma} ${k}`)
  assert.equal(sp(traduzir('pt-BR', 'contexto.detalhes.espacoValor', { usado: tokensCurtos('pt-BR', 84_000), total: tokensCurtos('pt-BR', 200_000) })), '84 mil de 200 mil tokens')
  assert.equal(sp(traduzir('en', 'contexto.detalhes.espacoValor', { usado: tokensCurtos('en', 84_000), total: tokensCurtos('en', 200_000) })), '84K of 200K tokens')
  const quando = new Date(2026, 10, 6, 14, 32).getTime()
  assert.equal(sp(traduzir('pt-BR', 'contexto.resumida.hoje', { hora: hora('pt-BR', quando) })), 'Resumida hoje às 14:32')
  assert.equal(sp(traduzir('en', 'contexto.resumida.hoje', { hora: hora('en', quando) })), 'Summarized today at 02:32 PM')
})

/** Hub falso: `donnaContext` responde `leituras[i]` (a última se acabar); `donnaContextCompact` aceita. */
function hub(leitura: () => RespostaHub, extra: { compactar?: () => Promise<unknown>; ler?: () => Promise<void> } = {}) {
  const chamadas: string[] = []
  const client = { call: async (method: string) => {
    chamadas.push(method)
    if (method === 'donnaContext') { await extra.ler?.(); return leitura() }
    return extra.compactar ? extra.compactar() : { accepted: true }
  } } as never
  return { chamadas, client }
}

test('P2-1: sucesso real do Hub (época nova, sem context) aparece como Comprimido, sem inventar tokens', async () => {
  let r: RespostaHub = resp('a', { context: ctx({ used: 60_000 }) })
  const { client } = hub(() => r)
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.resultado('a', null)?.fase, 'andamento')
  r = resp('a', { epoch: 1, context: undefined }) // contrato do Hub: uso da época anterior some, `context` inteiro omitido
  await m.ler('a', null)
  assert.equal(m.of('a', null), null)
  assert.deepEqual(m.resultado('a', null), { fase: 'feito', antes: 60_000, depois: null })
  assert.equal(m.emAndamento('a', null), false)
})

test('P2-1: corrida aceite × sucesso observado, nas quatro ordens', async () => {
  const novo = (over: Record<string, unknown>) => resp('a', over)
  const base = () => resp('a', { context: ctx({ used: 60_000, lastCompaction: { at: 5 } }) })
  const rodar = async (durante: RespostaHub | null, depoisDoAceite: RespostaHub | null) => {
    let r = base()
    let soltar!: (v: unknown) => void
    const { client } = hub(() => r, { compactar: () => new Promise((ok) => { soltar = ok }) })
    const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }), 15)
    await m.ler('a', null)
    const p = m.comprimir('a', null, 'c1')
    await tick()
    if (durante) { r = durante; await m.ler('a', null) } // poll vê o sucesso com o RPC ainda sem resposta
    soltar({ accepted: true })
    await p
    if (depoisDoAceite) { r = depoisDoAceite; await m.ler('a', null) }
    return m.resultado('a', null)
  }
  // 1) evento antes da resposta (sem medida nova) e 2) com medida nova + marcador novo
  assert.deepEqual(await rodar(novo({ epoch: 1, context: undefined }), null), { fase: 'feito', antes: 60_000, depois: null })
  assert.deepEqual(await rodar(novo({ context: ctx({ used: 20_000, lastCompaction: { at: 99 } }) }), null), { fase: 'feito', antes: 60_000, depois: 20_000 })
  // 3) resposta antes do evento
  assert.equal((await rodar(null, null))?.fase, 'andamento')
  assert.deepEqual(await rodar(null, novo({ epoch: 1, context: undefined })), { fase: 'feito', antes: 60_000, depois: null })
  // 4) só resposta: sem evento, termina sem confirmar (nunca "feito"); só evento sem pedido: nada a mostrar
  await new Promise((r) => setTimeout(r, 30))
  const m2 = new ModeloContexto(() => ({ client: hub(() => novo({ epoch: 1, context: undefined })).client, hostId: 'h', epoch: 1 }))
  await m2.ler('a', null)
  assert.equal(m2.resultado('a', null), null)
})

test('P2-2: consulta rejeitada ou Hub fora depois do aceite termina em aviso, nunca andamento infinito', async () => {
  let cair = false
  const client = { call: async (method: string) => { if (method === 'donnaContext') { if (cair) throw new Error('boom'); return resp('a') } return { accepted: true } } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }), 15)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  cair = true
  await m.ler('a', null)
  assert.equal(m.resultado('a', null)?.fase, 'andamento')
  let avisos = 0
  m.subscribe(() => { avisos++ })
  await new Promise((r) => setTimeout(r, 40))
  assert.equal(m.resultado('a', null)?.fase, 'incerto')
  assert.equal(m.emAndamento('a', null), false, 'botão liberado')
  assert.ok(avisos > 0, 'a view é avisada sem depender de leitura')
  assert.match(traduzir('pt-BR', 'contexto.aviso.incerto.titulo'), /Não deu para confirmar/)
})

test('P2-3: segundo clique durante a releitura atrasada não envia outro RPC', async () => {
  let solta!: () => void
  const trava = new Promise<void>((r) => { solta = r })
  let travar = false
  const { client, chamadas } = hub(() => resp('a'), { ler: async () => { if (travar) await trava } })
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  travar = true
  const primeiro = m.comprimir('a', null, 'c1')
  await tick()
  assert.equal(m.emAndamento('a', null), true, 'durante o RPC e a releitura')
  await m.comprimir('a', null, 'c1')
  assert.equal(chamadas.filter((c) => c === 'donnaContextCompact').length, 1, 'duplo clique no envio')
  solta(); await primeiro
  assert.equal(m.enviando('a', null), false)
  assert.equal(m.emAndamento('a', null), true, 'aceito e sem desfecho: continua bloqueado')
  await m.comprimir('a', null, 'c1')
  assert.equal(chamadas.filter((c) => c === 'donnaContextCompact').length, 1, 'clique depois do aceite')
  assert.equal(m.resultado('a', null)?.fase, 'andamento', 'o acompanhamento do primeiro não é apagado')
  assert.equal(motivoBloqueio({ busy: false, online: true, working: false, enviando: true }), 'enviando')
})

test('P2-4: troca de ACP/motor (scopeId) no mesmo Hub não conclui o pedido do escopo antigo', async () => {
  let r: RespostaHub = resp('a', { context: ctx({ used: 60_000 }) })
  const { client } = hub(() => r)
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  r = resp('a', { scopeId: 'B', epoch: 5, context: ctx({ used: 20_000, lastCompaction: { at: 99, byHost: true } }) })
  await m.ler('a', null)
  assert.deepEqual(m.resultado('a', null), { fase: 'mudou', antes: 60_000, depois: null })
  assert.equal(m.emAndamento('a', null), false)
})

test('P2-4: escopo trocado enquanto o RPC voava descarta o pedido; outra conexão/Hub não vê resultado', async () => {
  let r: RespostaHub = resp('a')
  let origem: Origem
  const { client } = hub(() => r, { compactar: async () => { r = resp('a', { scopeId: 'B' }); await m.ler('a', null); return { accepted: true } } })
  origem = { client, hostId: 'h', epoch: 1 }
  const m: ModeloContexto = new ModeloContexto(() => origem)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.resultado('a', null)?.fase, 'mudou')
  r = resp('a'); await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  origem = { ...origem, epoch: 2 }
  assert.equal(m.resultado('a', null), null, 'outra conexão')
})

// --- Redesenho do balão (dono, 16/11): a vista é função pura; um teste por estado do mockup. ---

const leitura = (over: Record<string, unknown> = {}) => conferirContexto(resp('a', { context: ctx({ used: 60_300, size: 475_000, trigger: 200_000, ...over }) }), pedido('a'))!.leitura!
const vista = (over: Partial<EntradaVista> = {}) => vistaBalao({ idioma: 'pt-BR', leitura: leitura(), online: true, working: false, enviando: false, emAndamento: false, recusa: null, resultado: null, ...over })

test('balão normal: percentual, marca onde a Mexerica resume sozinha, ajuda do botão e nenhum aviso', () => {
  const v = vista()
  assert.equal(v.pct, 13)
  assert.equal(v.marca, 42)
  assert.equal(v.faixa, 'sobra')
  assert.equal(v.detalhe, 'marca')
  assert.equal(v.aviso, null, 'sem aviso: a view mostra a frase do que o botão faz')
  assert.equal(v.botao, 'comprimir')
  assert.equal(v.desligado, false)
  assert.equal(v.alto, false)
  assert.equal(v.antesPct, null)
  // faixas e vermelho
  assert.equal(vista({ leitura: leitura({ used: 380_000 }) }).faixa, 'ficando')
  const quase = vista({ leitura: leitura({ used: 440_000 }) })
  assert.deepEqual([quase.faixa, quase.detalhe, quase.alto], ['quase', 'quase', true])
  // gatilho que não cabe na barra ou já passado: sem marca nem frase da marca
  assert.equal(vista({ leitura: leitura({ trigger: 475_000 }) }).marca, null)
  assert.equal(vista({ leitura: leitura({ used: 300_000 }) }).detalhe, null, 'já passou da marca: nada de "quando chegar perto"')
})

test('balão comprimindo: botão desligado como "Comprimindo…" e o aviso de andamento (pedido enviado ou aceito)', () => {
  const enviando = vista({ enviando: true, emAndamento: true })
  assert.deepEqual([enviando.botao, enviando.desligado, enviando.aviso?.id], ['comprimindo', true, 'andamento'])
  const aceito = vista({ emAndamento: true, resultado: { fase: 'andamento', antes: 60_300, depois: null } })
  assert.deepEqual([aceito.botao, aceito.desligado, aceito.aviso?.id, aceito.aviso?.tom], ['comprimindo', true, 'andamento', 'neutro'])
})

test('balão pronto com medida: de X para Y, percentuais antes e depois, barra com a parte listrada', () => {
  const v = vista({ leitura: leitura({ used: 22_000 }), resultado: { fase: 'feito', antes: 60_300, depois: 22_000 } })
  assert.equal(v.aviso?.id, 'feito')
  assert.equal(v.aviso?.tom, 'ok')
  assert.deepEqual(v.aviso?.vars && { ...v.aviso.vars, antes: sp(v.aviso.vars.antes), depois: sp(v.aviso.vars.depois) }, { antes: '60,3 mil', depois: '22 mil', de: '13%', para: '5%' })
  assert.equal(v.antesPct, 13)
  assert.equal(v.pct, 5)
  assert.equal(v.pronto, true)
  assert.equal(v.faixa, 'depois')
  assert.equal(v.desligado, false, 'passado o pedido, pode comprimir de novo')
})

test('balão pronto sem medida: sem inventar tokens nem percentual, sem barra listrada', () => {
  const v = vista({ resultado: { fase: 'feito', antes: 60_300, depois: null } })
  assert.equal(v.aviso?.id, 'feitoSemMedida')
  assert.equal(v.aviso?.vars, undefined)
  assert.equal(v.antesPct, null)
  assert.equal(v.pronto, false)
})

test('balão: sem efeito é neutro, não confirmado pede atenção, "mudou" é neutro (nada em vermelho)', () => {
  const de = (fase: 'semEfeito' | 'incerto' | 'mudou') => vista({ resultado: { fase, antes: 60_300, depois: null } }).aviso
  assert.deepEqual([de('semEfeito')?.id, de('semEfeito')?.tom], ['semEfeito', 'neutro'])
  assert.deepEqual([de('incerto')?.id, de('incerto')?.tom], ['incerto', 'atencao'])
  assert.deepEqual([de('mudou')?.id, de('mudou')?.tom], ['mudou', 'neutro'])
})

const comRecusa = async (erro: string, origem: { epoch: number } = { epoch: 1 }) => {
  const client = { call: async (method: string, body: { botId: string }) => { if (method === 'donnaContext') return resp(body.botId); throw new Error(erro) } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: origem.epoch }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  return m
}

test('recusa "resumida há pouco": neutra, botão desligado e expira sozinha com o resultado (VALIDADE_MS)', async () => {
  const real = Date.now
  let agora = real()
  Date.now = () => agora
  try {
    const m = await comRecusa('context_repeat_guard')
    const v = () => vista({ recusa: m.recusa('a', null) })
    assert.deepEqual([v().aviso?.id, v().aviso?.tom, v().desligado, v().botao], ['repetida', 'neutro', true, 'comprimir'])
    agora += VALIDADE_MS - 1
    assert.equal(m.recusa('a', null), 'repetida', 'ainda vale')
    agora += 2
    assert.equal(m.recusa('a', null), null, 'expirou')
    assert.deepEqual([v().aviso, v().desligado], [null, false], 'sem aviso e o botão volta')
  } finally { Date.now = real }
})

test('as outras recusas não expiram; só "antigo" também desliga o botão', async () => {
  const real = Date.now
  let agora = real()
  Date.now = () => agora
  try {
    for (const [erro, id, desligado] of [['context_busy', 'ocupada', false], ['context_engine_changed', 'mudou', false], ['context_unavailable', 'indisponivel', false], ['timeout', 'falha', false]] as const) {
      const m = await comRecusa(erro)
      agora += VALIDADE_MS * 3
      const v = vista({ recusa: m.recusa('a', null) })
      assert.deepEqual([v.aviso?.id, v.desligado], [id, desligado], erro)
      agora = real()
    }
    const antigo = vista({ recusa: 'antigo' })
    assert.deepEqual([antigo.aviso?.id, antigo.aviso?.tom, antigo.desligado], ['antigo', 'atencao', true])
  } finally { Date.now = real }
})

test('falha de verdade: erro com "Tentar de novo" (botão ligado); outra recusa mantém "Comprimir agora"', async () => {
  const m = await comRecusa('timeout')
  const v = vista({ recusa: m.recusa('a', null) })
  assert.deepEqual([v.aviso?.id, v.aviso?.tom, v.aviso?.icone, v.botao, v.desligado], ['falha', 'erro', 'alerta', 'tentar', false])
  assert.equal(vista({ recusa: 'mudou' }).botao, 'comprimir')
  // clicar de novo (aceito) limpa a falha
  const aceito = { call: async (method: string, body: { botId: string }) => method === 'donnaContext' ? resp(body.botId) : { accepted: true } } as never
  const m2 = new ModeloContexto(() => ({ client: aceito, hostId: 'h', epoch: 1 }))
  await m2.ler('a', null)
  assert.equal(m2.recusa('a', null), null)
})

test('bloqueios viram aviso neutro com o motivo escrito; automático pausado pede atenção e deixa o botão ligado', () => {
  const turno = vista({ working: true })
  assert.deepEqual([turno.aviso?.id, turno.aviso?.tom, turno.desligado], ['turno', 'neutro', true])
  assert.deepEqual([vista({ online: false }).aviso?.id, vista({ online: false }).desligado], ['semConexao', true])
  assert.deepEqual([vista({ leitura: leitura({ canCompact: false }) }).aviso?.id, vista({ leitura: leitura({ canCompact: false }) }).desligado], ['semMotor', true])
  const pausado = vista({ leitura: leitura({ automaticPaused: true, automaticFailures: 3 }) })
  assert.deepEqual([pausado.aviso?.id, pausado.aviso?.tom, pausado.desligado, pausado.marca, pausado.detalhe], ['pausado', 'atencao', false, null, null])
  assert.equal(vista({ leitura: leitura({ automaticPaused: true }), working: true }).aviso?.id, 'turno', 'o motivo de estar desligado vem antes do aviso de pausa')
})

test('Hub antigo sem donnaContext: o medidor fica avisando (não some); a demo da web continua sem aviso', async () => {
  const sem = (message: string) => ({ call: async () => { throw Object.assign(new Error(message), { status: message === 'unknown method' ? 404 : undefined }) } }) as never
  const o1 = { client: sem('unknown method'), hostId: 'h', epoch: 1 }
  const antigo = new ModeloContexto(() => o1)
  assert.equal(antigo.hubAntigo(), false, 'antes de perguntar')
  await antigo.ler('a', null)
  assert.deepEqual([antigo.suportado(), antigo.hubAntigo(), antigo.of('a', null)], [false, true, null])
  const o2 = { client: sem('Not available in the web demo. Install Mexerica to use this.'), hostId: 'h', epoch: 1 }
  const demo = new ModeloContexto(() => o2)
  await demo.ler('a', null)
  assert.deepEqual([demo.suportado(), demo.hubAntigo()], [false, false])
  const a = avisoSemSuporte()
  assert.deepEqual([a.id, a.tom], ['semSuporte', 'atencao'])
  // outra conexão (Hub atualizado) volta a perguntar
  let origem = { client: sem('unknown method'), hostId: 'h', epoch: 1 }
  const m = new ModeloContexto(() => origem)
  await m.ler('a', null)
  origem = { ...origem, epoch: 2 }
  assert.equal(m.hubAntigo(), false)
})

test('"Resumida hoje às 12:39", de outro dia com data, e "ainda não foi resumida"', () => {
  const agora = new Date(2026, 10, 16, 15, 0).getTime()
  const hoje = resumidaEm('pt-BR', { at: new Date(2026, 10, 16, 12, 39).getTime(), byHost: true }, agora)
  assert.deepEqual([hoje.id, sp(hoje.vars!.hora)], ['hoje', '12:39'])
  const antes = resumidaEm('pt-BR', { at: new Date(2026, 10, 12, 9, 5).getTime(), byHost: false }, agora)
  assert.equal(antes.id, 'em')
  assert.equal(sp(antes.vars!.data), '12 de nov.')
  assert.equal(resumidaEm('pt-BR', null, agora).id, 'nunca')
})

test('textos do balão: todo aviso, "resumida" e a linguagem leiga do primeiro nível existem nos seis idiomas', () => {
  const ids = ['andamento', 'feito', 'feitoSemMedida', 'semEfeito', 'incerto', 'mudou', 'repetida', 'ocupada', 'indisponivel', 'falha', 'antigo', 'semMotor', 'semConexao', 'turno', 'pausado', 'semSuporte']
  const chaves = [...ids.flatMap((i) => [`contexto.aviso.${i}.titulo`, `contexto.aviso.${i}.detalhe`]), 'contexto.resumida.hoje', 'contexto.resumida.em', 'contexto.resumida.nunca',
    'contexto.estado.sobra', 'contexto.estado.ficando', 'contexto.estado.quase', 'contexto.estado.depois', 'contexto.estado.marca', 'contexto.estado.quaseDetalhe', 'contexto.ajuda']
  for (const [idioma, cat] of Object.entries(CATALOGOS)) for (const k of chaves) assert.ok((cat as Record<string, string>)[k]?.trim(), `${idioma} ${k}`)
  // Primeiro nível sem jargão em pt-BR: token, cache, scope e motor ficam só em Ver detalhes.
  const pt = CATALOGOS['pt-BR'] as Record<string, string>
  for (const k of chaves) assert.doesNotMatch(pt[k], /token|cache|scope|motor|\bHub\b/i, k)
  // Mesmos parâmetros em todo idioma (um texto sem {n} quebraria em silêncio).
  const nomes = (x: string) => [...x.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join()
  for (const [idioma, cat] of Object.entries(CATALOGOS)) for (const k of chaves) assert.equal(nomes((cat as Record<string, string>)[k]), nomes(pt[k]), `${idioma} ${k}`)
})


test('recusa repetida notifica ao vencer mesmo com leituras falhando; descarte cancela o relógio', async () => {
  let agora = 0
  const tarefas = new Map<number, () => void>()
  let id = 0
  const relogio = {
    agora: () => agora,
    agendar: ((f: () => void) => { tarefas.set(++id, f); return id }) as unknown as typeof setTimeout,
    cancelar: ((n: number) => tarefas.delete(n)) as unknown as typeof clearTimeout,
  }
  let falhar = false
  const client = { call: async (metodo: string) => {
    if (metodo !== 'donnaContext') { falhar = true; throw new Error('context_repeat_guard') }
    if (falhar) throw new Error('offline')
    return resp('a')
  } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }), 90_000, relogio)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  await Promise.resolve()
  let notificacoes = 0
  m.subscribe(() => notificacoes++)
  assert.equal(vista({ recusa: m.recusa('a', null) }).desligado, true)
  agora = VALIDADE_MS
  for (const f of tarefas.values()) f()
  await Promise.resolve()
  assert.equal(notificacoes, 1)
  assert.deepEqual([m.recusa('a', null), vista({ recusa: m.recusa('a', null) }).desligado], [null, false])
  falhar = false
  await m.comprimir('a', null, 'c1')
  m.descartar('a', null)
  assert.equal(tarefas.size, 1) // o relógio já executado é removido pelo escalonador real
})

test('sem suporte mantém botão desligado quando a leitura posterior falha', async () => {
  let falhar = false
  const client = { call: async (metodo: string) => {
    if (metodo !== 'donnaContext') { falhar = true; throw new Error('context_cannot_compact') }
    if (falhar) throw new Error('offline')
    return resp('a')
  } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }))
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(m.of('a', null)?.canCompact, true)
  assert.equal(vista({ recusa: m.recusa('a', null) }).desligado, true)
  falhar = false
  await m.ler('a', null)
  assert.equal(m.recusa('a', null), null)
})

test('descarte da conversa cancela relógio do resultado aceito', async () => {
  const tarefas = new Map<number, () => void>()
  let id = 0 // um número por relógio armado: o pedido em voo também arma o seu e o desarma ao responder
  const relogio = {
    agora: () => 0,
    agendar: ((f: () => void) => { tarefas.set(++id, f); return id }) as unknown as typeof setTimeout,
    cancelar: ((n: number) => tarefas.delete(n)) as unknown as typeof clearTimeout,
  }
  const client = { call: async (metodo: string) => metodo === 'donnaContext' ? resp('a') : { accepted: true } } as never
  const m = new ModeloContexto(() => ({ client, hostId: 'h', epoch: 1 }), 90_000, relogio)
  await m.ler('a', null)
  await m.comprimir('a', null, 'c1')
  assert.equal(tarefas.size, 1)
  m.descartar('a', null)
  assert.equal(tarefas.size, 0)
  assert.equal(m.resultado('a', null), null)
})


test('bloqueio atual explica botão desligado antes de resultado ou recusa anterior', () => {
  assert.equal(vista({ working: true, resultado: { fase: 'feito', antes: 60_000, depois: 22_000 } }).aviso?.id, 'turno')
  assert.equal(vista({ working: true, recusa: 'falha' }).aviso?.id, 'turno')
  assert.equal(vista({ online: false, recusa: 'repetida' }).aviso?.id, 'semConexao')
})

test('partes do contexto conferidas; o que o motor não informa nunca ganha número', () => {
  const parte = (id: string, tokens: number | null, origin = 'estimado') => ({ id, origin, chars: null, tokens })
  const bruto = { charsPerToken: 4, items: [parte('fixo', 7_000), parte('memoria', 300), parte('skills', 0), parte('ferramentas', null, 'naoInformado'), parte('conversa', 76_700)] }
  const l = conferirContexto(resp('a', { context: ctx({ parts: bruto }) }), pedido('a'))?.leitura
  assert.deepEqual(l?.partes?.itens.map((p) => [p.id, p.origem, p.tokens]), [['fixo', 'estimado', 7_000], ['memoria', 'estimado', 300], ['skills', 'estimado', 0], ['ferramentas', 'naoInformado', null], ['conversa', 'estimado', 76_700]])
  assert.equal(l?.partes?.caracteresPorToken, 4)
  // Hub antigo (sem parts) segue medindo o total; o balão diz que falta a divisão.
  assert.equal(conferirContexto(resp('a'), pedido('a'))?.leitura?.partes, null)
  // Número vindo como "não informado" é descartado; qualquer desvio do contrato descarta a divisão inteira.
  const naoInformado = { ...bruto, items: [{ ...parte('fixo', 999, 'naoInformado') }, ...bruto.items.slice(1)] }
  assert.equal(conferirContexto(resp('a', { context: ctx({ parts: naoInformado }) }), pedido('a'))?.leitura?.partes?.itens[0]?.tokens, null)
  for (const ruim of [{ ...bruto, items: bruto.items.slice(1) }, { ...bruto, items: [...bruto.items].reverse() }, { ...bruto, charsPerToken: 0 }, { ...bruto, items: [parte('fixo', -1), ...bruto.items.slice(1)] }, { ...bruto, items: [parte('fixo', 1.5), ...bruto.items.slice(1)] }, { ...bruto, items: [parte('fixo', 5, 'medido'), ...bruto.items.slice(1)] }, 'texto', null]) {
    assert.equal(conferirContexto(resp('a', { context: ctx({ parts: ruim }) }), pedido('a'))?.leitura?.partes, null, JSON.stringify(ruim))
  }
})
