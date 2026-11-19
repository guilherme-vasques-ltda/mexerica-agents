import assert from 'node:assert/strict'
import test from 'node:test'
import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { CatalogSkill, InstalledSkillV2 } from '../../../store/fork-skills.ts'
import { CatalogoStore } from './catalogo-store.ts'
import { capturarRemocao, estadoDoItem, itemDoCatalogo, type AlvoRemocao } from './regras.ts'
import { textoDe, frases as textos, type Frase } from './textos.ts'

// D454 (fatia 8b): a loja guarda frases; o texto pt-BR, sem os isolantes bidi dos nomes, é o de antes.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : f)

// Digests sintéticos em hex de 64 (o wrapper forte recusa outro formato).
const D1 = 'd1'.repeat(32)
const D0 = 'd0'.repeat(32)
const NS = '11111111-1111-4111-8111-111111111111'

const item = (patch: Partial<CatalogSkill> = {}): CatalogSkill => ({
  id: 'frontend-design', source: 'frontend-design', name: 'Design de interfaces', description: 'Interfaces com identidade.',
  category: 'Design', aliases: [], origin: { repo: 'exemplo/skills', revision: 'a'.repeat(40), path: 'skills/frontend-design', sourceDigest: 'b'.repeat(64) },
  license: 'Apache-2.0', availability: 'text', reason: '', requirements: [], losses: [], adaptation: null, digest: D1,
  files: [], installable: true, installed: false, installedId: null, readiness: null, ...patch,
})
const recibo = (catalogId: string, digest: string) => ({
  schema: 1 as const, catalogId, digest, origin: item().origin, adaptation: null, license: 'MIT', files: [],
  result: 'imported' as const, coverage: 'pinnedPassiveTextOnly' as const, limitations: [],
})
/** Uma instalada como o Hub R7 manda: encarnação `namespace:sequência` e conteúdo conferido. */
const inst = (seq: number, patch: Partial<InstalledSkillV2> & Pick<InstalledSkillV2, 'id' | 'name'>): InstalledSkillV2 => ({
  description: '', source: '', path: '', receipt: null, installationId: `${NS}:${seq}`, contentStatus: 'verified', ...patch,
})
const alvo = (s: InstalledSkillV2): AlvoRemocao => capturarRemocao(s)!

/**
 * Hub falso só com os RPCs do contrato R7: envelopes `skillProtocol: 2` (leituras V1 compatíveis
 * também levam a prova), encarnação nova a cada instalação e remoção estrita como no Host — pedido
 * antigo `{id}`/`{id,expectedDigest}` é recusado; encarnação ou recibo divergente, `stale`.
 * Cada chamada espera até o teste soltar, se pedido.
 */
function hubFalso(estado: { itens: CatalogSkill[]; instaladas: InstalledSkillV2[] }) {
  const chamadas: [string, unknown][] = []
  const presos: { metodo: string; soltar: () => void; falhar: (e: Error) => void }[] = []
  let prender: string | null = null
  let sequencia = 100
  const client: SkillClient = {
    async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      chamadas.push([metodo, corpo])
      if (prender === metodo) {
        prender = null
        await new Promise<void>((soltar, falhar) => presos.push({ metodo, soltar, falhar }))
      }
      if (metodo === 'marketSkills') return { skillProtocol: 2, items: structuredClone(estado.itens) } as T
      if (metodo === 'skills') return { skillProtocol: 2, items: structuredClone(estado.instaladas) } as T
      if (metodo === 'installSkill') {
        const { catalogId, expectedInstallationId } = corpo as { catalogId: string; expectedInstallationId?: string }
        if (expectedInstallationId !== undefined) {
          // D306: como no Host, só a encarnação vista é reposta pela versão do recibo.
          const atual = estado.instaladas.find((s) => s.receipt?.catalogId === catalogId)
          if (!atual || atual.installationId !== expectedInstallationId) throw new Error('skill_installation_stale')
          atual.contentStatus = 'verified'
          return { skillProtocol: 2, skill: structuredClone(atual), alreadyInstalled: false } as T
        }
        estado.itens = estado.itens.map((i) => (i.id === catalogId ? { ...i, installed: true, installedId: `${catalogId}-1` } : i))
        estado.instaladas.push(inst(++sequencia, { id: `${catalogId}-1`, name: catalogId, receipt: recibo(catalogId, D1) }))
        return { skillProtocol: 2, skill: estado.instaladas.at(-1), alreadyInstalled: false } as T
      }
      if (metodo === 'removeSkill') {
        const pedido = corpo as { skillProtocol?: number; id: string; expectedInstallationId?: string; expectedReceipt?: { kind: string; catalogId?: string; digest?: string } }
        if (pedido.skillProtocol !== 2 || !pedido.expectedInstallationId || !pedido.expectedReceipt) throw new Error('skill_installation_unsupported')
        const atual = estado.instaladas.find((s) => s.id === pedido.id)
        if (atual) {
          const recibo = pedido.expectedReceipt.kind === 'none' ? null : pedido.expectedReceipt
          const mesmoRecibo = recibo === null ? atual.receipt === null : atual.receipt?.catalogId === recibo.catalogId && atual.receipt?.digest === recibo.digest
          if (atual.installationId !== pedido.expectedInstallationId || !mesmoRecibo) throw new Error('skill_installation_stale')
        }
        estado.instaladas = estado.instaladas.filter((s) => s.id !== pedido.id)
        estado.itens = estado.itens.map((i) => (i.installedId === pedido.id ? { ...i, installed: false, installedId: null } : i))
        return { skillProtocol: 2 } as T
      }
      throw new Error(`método inesperado: ${metodo}`)
    },
  }
  return { client, chamadas, presos, prender: (m: string) => { prender = m }, estado }
}

const esperar = () => new Promise((r) => setTimeout(r, 0))

test('instalar envia catalogId e digest uma vez só e só mostra instalada depois de reler o Hub', async () => {
  const hub = hubFalso({ itens: [item(), item({ id: 'outra', name: 'Outra' })], instaladas: [] })
  let mudou = 0
  const loja = new CatalogoStore(async () => hub.client, () => { mudou++ })
  loja.sincronizar('hub-a')
  await loja.carregar()
  hub.prender('installSkill')
  const primeiro = loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  void loja.instalar(loja.itens![0]!, loja.capturarIntencao()) // clique duplo
  await esperar()
  assert.equal(loja.pedido('frontend-design')?.fase, 'enviando')
  assert.equal(loja.pedido('outra'), null, 'os outros itens continuam livres')
  hub.prender('marketSkills')
  hub.presos.shift()!.soltar()
  await esperar()
  assert.equal(loja.pedido('frontend-design')?.fase, 'conferindo', 'ocupado até o Hub confirmar')
  assert.equal(loja.itens![0]!.installed, false, 'a resposta da mutação não marca instalada')
  assert.equal(loja.instaladaAgora, null, 'nem o aviso de instalada aparece antes de reler')
  hub.presos.shift()!.soltar()
  await primeiro
  assert.equal(loja.pedido('frontend-design'), null)
  assert.equal(loja.itens![0]!.installed, true)
  assert.equal(loja.instaladaAgora, 'Design de interfaces')
  assert.equal(mudou, 1)
  assert.deepEqual(hub.chamadas.filter(([m]) => m === 'installSkill'), [['installSkill', { skillProtocol: 2, catalogId: 'frontend-design', expectedDigest: D1 }]])
  const leituras = hub.chamadas.filter(([m]) => m === 'skills' || m === 'marketSkills')
  assert.ok(leituras.length >= 4 && leituras.every(([, c]) => JSON.stringify(c) === '{"skillProtocol":2}'), 'leituras sempre pelo protocolo forte')
})

test('resposta e erro atrasados do Hub anterior não mexem no Hub novo', async () => {
  const a = hubFalso({ itens: [item()], instaladas: [] })
  const b = hubFalso({ itens: [item({ id: 'so-no-b', name: 'Só no B' })], instaladas: [] })
  let atual = a
  const loja = new CatalogoStore(async () => atual.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  a.prender('installSkill')
  const antigo = loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  await esperar()
  atual = b
  loja.sincronizar('hub-b')
  await loja.carregar()
  assert.deepEqual(loja.itens!.map((i) => i.id), ['so-no-b'])
  b.prender('installSkill')
  const novo = loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  await esperar()
  a.presos.shift()!.falhar(new Error('install_failed: Hub anterior'))
  await antigo
  assert.equal(loja.falhas.size, 0, 'erro do Hub anterior descartado')
  assert.equal(loja.pedido('so-no-b')?.fase, 'enviando', 'o pedido novo continua ocupado')
  assert.deepEqual(loja.itens!.map((i) => i.id), ['so-no-b'])
  b.presos.shift()!.soltar()
  await novo
  assert.equal(loja.itens![0]!.installed, true)
})

test('remover manda encarnação e recibo esperado capturados (catálogo e sem recibo); leitura velha não ressuscita', async () => {
  const hub = hubFalso({
    itens: [item({ installed: true, installedId: 'frontend-design-1' })],
    instaladas: [
      inst(1, { id: 'frontend-design-1', name: 'Design de interfaces', receipt: recibo('frontend-design', D1) }),
      inst(2, { id: 'pdf', name: 'pdf', description: 'legado', source: 'pdf' }),
    ],
  })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  hub.prender('skills')
  const velha = loja.carregar() // leitura que começou antes da remoção
  await esperar()
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  hub.presos.shift()!.soltar()
  await velha
  assert.deepEqual(loja.instaladas!.map((s) => s.id), ['pdf'])
  assert.equal(loja.itens![0]!.installed, false)
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  assert.deepEqual(hub.chamadas.filter(([m]) => m === 'removeSkill').map(([, c]) => c), [
    { skillProtocol: 2, id: 'frontend-design-1', expectedInstallationId: `${NS}:1`, expectedReceipt: { kind: 'catalog', catalogId: 'frontend-design', digest: D1 } },
    { skillProtocol: 2, id: 'pdf', expectedInstallationId: `${NS}:2`, expectedReceipt: { kind: 'none' } },
  ])
  assert.deepEqual(loja.instaladas, [])
})

test('recusa comprovada antes do efeito guarda o motivo e libera tentar de novo; erro de leitura mantém a lista', async () => {
  const hub = hubFalso({ itens: [item()], instaladas: [] })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  hub.prender('installSkill')
  const tentativa = loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  await esperar()
  // R7: só recusa nomeada antes do efeito é falha; erro genérico depois do envio é incerto (teste abaixo).
  hub.presos.shift()!.falhar(new Error('forbidden'))
  await tentativa
  assert.equal(pt(loja.falhas.get('frontend-design')), 'forbidden')
  assert.equal(loja.incertos.size, 0)
  assert.equal(loja.pedido('frontend-design'), null)
  assert.equal(loja.itens![0]!.installed, false)
  hub.prender('marketSkills')
  const leitura = loja.carregar()
  await esperar()
  hub.presos.shift()!.falhar(new Error('Hub ocupado'))
  await leitura
  assert.equal(pt(loja.erro), 'Hub ocupado')
  assert.equal(loja.itens?.length, 1, 'erro não vira lista vazia')
})

test('item fora do contrato não vira instalável; estado nunca casa por nome', async () => {
  const hub = hubFalso({ itens: [{ source: 'pdf', name: 'pdf', description: 'antigo', installed: false } as unknown as CatalogSkill], instaladas: [] })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  assert.equal(loja.hubAntigo, true)
  assert.equal(loja.semProtocolo, false)
  assert.deepEqual(loja.itens, [])
  assert.equal(itemDoCatalogo(item()), true)

  const legadoMesmoNome = inst(1, { id: 'frontend-design', name: 'Design de interfaces' })
  assert.equal(estadoDoItem(item(), [legadoMesmoNome]).tipo, 'instalavel')
  const anterior = inst(2, { id: 'fd-0', name: 'Design de interfaces', receipt: recibo('frontend-design', D0) })
  assert.equal(estadoDoItem(item(), [anterior]).tipo, 'outraVersao')
  assert.equal(estadoDoItem(item({ availability: 'licenseUnavailable', installable: true }), []).tipo, 'bloqueada')
})

for (const [nome, resposta] of [['com lista', [{ id: 'pdf', name: 'pdf', description: '', source: 'pdf', path: '', receipt: null }]], ['vazio', []]] as const) {
  test(`Hub sem o protocolo forte (${nome}): sem lista, sem fallback V1 e nenhuma ação`, async () => {
    const chamadas: [string, unknown][] = []
    const retida = inst(1, { id: 'pdf', name: 'pdf', source: 'pdf' })
    const antigo: SkillClient = { async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      chamadas.push([metodo, corpo])
      // Como o Hub 6dc: ignora o corpo das leituras e responde sem a prova de versão.
      if (metodo === 'skills' || metodo === 'marketSkills') return { items: structuredClone(resposta) } as T
      throw new Error('Intenção de remoção inválida')
    } }
    const loja = new CatalogoStore(async () => antigo)
    loja.sincronizar('hub-a')
    await loja.carregar()
    assert.equal(loja.semProtocolo, true)
    assert.equal(loja.itens, null)
    assert.equal(loja.instaladas, null)
    assert.equal(loja.erro, null, 'não é erro passageiro')
    // Callbacks retidos de uma tela anterior também não mutam nada neste Hub.
    const origem = loja.capturarIntencao()
    await loja.instalar(item(), origem)
    await loja.remover(alvo(retida), origem)
    assert.equal(await loja.escrever({ name: 'Nota', description: '', instructions: 'Texto.' }, origem), 'falhou')
    assert.ok(chamadas.length > 0 && chamadas.every(([m, c]) => (m === 'skills' || m === 'marketSkills') && JSON.stringify(c) === '{"skillProtocol":2}'),
      'só leituras fortes, nunca repetidas no formato antigo, e nenhuma mutação')
  })
}

for (const operacao of ['instalar', 'remover'] as const) {
  test(`${operacao}: intenção que espera ready não passa ao Hub seguinte`, async () => {
    const manual = (hub: string) => inst(hub === 'A' ? 1 : 2, { id: 'documento-local', name: 'Documento local', description: `Texto ${hub}`, source: 'custom' })
    const a = hubFalso({ itens: [item()], instaladas: [manual('A')] })
    const b = hubFalso({ itens: [item()], instaladas: [manual('B')] })
    let atual = a
    let soltar!: () => void
    let espera: Promise<void> | null = null
    const loja = new CatalogoStore(async () => { if (espera) await espera; return atual.client })
    loja.sincronizar('hub-a')
    await loja.carregar()
    espera = new Promise<void>((r) => { soltar = r })
    const antiga = operacao === 'instalar' ? loja.instalar(loja.itens![0]!, loja.capturarIntencao()) : loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
    await esperar()
    assert.equal(a.chamadas.filter(([m]) => m === 'installSkill' || m === 'removeSkill').length, 0)
    atual = b
    loja.sincronizar('hub-b')
    espera = null
    soltar()
    await antiga
    await loja.carregar()
    assert.deepEqual(b.chamadas.filter(([m]) => m === 'installSkill' || m === 'removeSkill'), [])
    assert.equal(b.estado.instaladas[0]!.description, 'Texto B')
    assert.equal(loja.pedidos.size, 0)
  })
}

test('a troca viva bloqueia o envio mesmo antes de sincronizar a loja; cliente substituído exige retry', async () => {
  const a = hubFalso({ itens: [item()], instaladas: [] })
  const b = hubFalso({ itens: [item()], instaladas: [] })
  let contexto = { hub: 'hub-a', client: a.client }
  let obter = async () => a.client
  const loja = new CatalogoStore(() => obter(), () => {}, () => contexto)
  loja.sincronizar('hub-a')
  await loja.carregar()
  let soltar!: () => void
  obter = () => new Promise((r) => { soltar = () => r(b.client) })
  const antiga = loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  contexto = { hub: 'hub-b', client: b.client } // o effect React ainda não rodou
  soltar()
  await antiga
  assert.deepEqual(b.chamadas, [], 'nenhum efeito antes de sincronizar')
  assert.equal(loja.capturarIntencao(), null, 'a lista de A ainda visível não cria intenção no B')

  obter = async () => b.client
  loja.sincronizar('hub-b')
  await loja.carregar()
  obter = async () => a.client // ready devolveu um cliente substituído
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.equal(a.chamadas.some(([m]) => m === 'installSkill'), false)
  assert.equal(loja.pedidos.size, 0, 'não prende o botão')
  assert.match(pt(loja.falhas.get('frontend-design'))!, /conexão.*mudou/)
  obter = async () => b.client
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.equal(b.estado.itens[0]!.installed, true, 'retry explícito no Hub atual funciona')
})

test('a confirmação mantém Hub e época de abertura, inclusive ao voltar ao mesmo Hub', async () => {
  const skill = (texto: string) => inst(texto === 'A' ? 1 : 2, { id: 'manual', name: 'Manual', description: texto, source: 'custom' })
  const a = hubFalso({ itens: [], instaladas: [skill('A')] })
  const b = hubFalso({ itens: [], instaladas: [skill('B')] })
  let atual = a
  const loja = new CatalogoStore(async () => atual.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const origem = loja.capturarIntencao()!
  const selecionada = alvo(loja.instaladas![0]!)
  atual = b
  loja.sincronizar('hub-b')
  await loja.carregar()
  await loja.remover(selecionada, origem) // callback do diálogo já fechado em A
  assert.equal(b.chamadas.some(([m]) => m === 'removeSkill'), false)
  assert.equal(b.estado.instaladas[0]!.description, 'B')
  assert.equal(loja.pedidos.size, 0)
  assert.equal(loja.aviso, null)
  atual = a
  loja.sincronizar('hub-a')
  await loja.carregar()
  await loja.remover(selecionada, origem)
  assert.equal(a.chamadas.some(([m]) => m === 'removeSkill'), false, 'mesmo hostId não revive a época antiga')
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  assert.equal(a.estado.instaladas.length, 0, 'nova confirmação remove normalmente')
})


test('remover sem origem explícita não transforma objeto antigo em intenção do Hub atual', async () => {
  const skill = inst(1, { id: 'manual', name: 'Manual', description: 'Texto B', source: 'custom' })
  const b = hubFalso({ itens: [], instaladas: [skill] })
  const loja = new CatalogoStore(async () => b.client)
  loja.sincronizar('hub-b')
  await loja.carregar()
  // Consumidor JS/callback antigo sem segundo argumento, ou com a skill crua: a fronteira nega em execução.
  await (loja.remover as (a: unknown) => Promise<void>)(alvo(skill))
  await (loja.remover as (a: unknown, o: unknown) => Promise<void>)(skill, loja.capturarIntencao())
  assert.equal(b.chamadas.some(([m]) => m === 'removeSkill'), false)
  assert.equal(b.estado.instaladas.length, 1)
  assert.equal(loja.pedidos.size, 0)
  assert.equal(loja.aviso, null)
})

// R8: identidade de encarnação, conteúdo e detalhes no protocolo forte.

test('reinstalação idêntica na mesma ID: vista, a confirmação não envia nada; não vista, o Hub recusa como stale e nada é removido', async () => {
  const manual = (seq: number) => inst(seq, { id: 'nota', name: 'Nota', description: 'Mesmo texto', source: 'custom', path: '/synthetic/nota/SKILL.md' })
  const hub = hubFalso({ itens: [], instaladas: [manual(1)] })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const vista = alvo(loja.instaladas![0]!)
  const origem = loja.capturarIntencao()
  // Outro cliente remove e escreve a mesma manual de novo: registro idêntico, outra encarnação.
  hub.estado.instaladas = [manual(2)]

  // Não vista: o pedido sai com a encarnação antiga e o Hub recusa antes do efeito.
  await loja.remover(vista, origem)
  const remocoes = () => hub.chamadas.filter(([m]) => m === 'removeSkill').map(([, c]) => c)
  assert.deepEqual(remocoes(), [{ skillProtocol: 2, id: 'nota', expectedInstallationId: `${NS}:1`, expectedReceipt: { kind: 'none' } }])
  assert.equal(pt(loja.falhas.get('rm:nota')), pt(textos.instalacaoMudou))
  assert.deepEqual(hub.estado.instaladas.map((s) => s.installationId), [`${NS}:2`], 'a instalação nova continua')
  assert.deepEqual(loja.instaladas!.map((s) => s.installationId), [`${NS}:2`], 'a lista foi relida do mesmo Hub')

  // Vista: a lista já mostra a encarnação nova, então a confirmação antiga não envia nada.
  await loja.remover(vista, loja.capturarIntencao())
  assert.equal(remocoes().length, 1, 'nenhum RPC novo')
  assert.equal(pt(loja.falhas.get('rm:nota')), pt(textos.instalacaoMudou))
  assert.equal(loja.pedidos.size, 0)

  // Gesto novo na linha de agora remove a encarnação nova.
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  assert.deepEqual(remocoes().at(-1), { skillProtocol: 2, id: 'nota', expectedInstallationId: `${NS}:2`, expectedReceipt: { kind: 'none' } })
  assert.deepEqual(hub.estado.instaladas, [])
})

test('D306: alterada ou indisponível se remove pela encarnação vista; outra encarnação não envia nada', async () => {
  for (const contentStatus of ['changed', 'unavailable'] as const) {
    assert.deepEqual(capturarRemocao(inst(1, { id: 'x', name: 'X', contentStatus }))?.pedido, { id: 'x', expectedInstallationId: `${NS}:1`, expectedReceipt: { kind: 'none' } })
  }
  const hub = hubFalso({ itens: [], instaladas: [inst(1, { id: 'x', name: 'X', contentStatus: 'changed' })] })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const vista = alvo(loja.instaladas![0]!)
  hub.estado.instaladas = [{ ...hub.estado.instaladas[0]!, installationId: `${NS}:2` }]
  await loja.carregar()
  await loja.remover(vista, loja.capturarIntencao())
  assert.equal(hub.chamadas.some(([m]) => m === 'removeSkill'), false)
  assert.equal(pt(loja.falhas.get('rm:x')), pt(textos.instalacaoMudou))
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  assert.deepEqual(hub.estado.instaladas, [])
})

test('D306: reinstalar curada alterada leva a encarnação vista e só confirma com a lista conferida', async () => {
  const curada = inst(3, { id: 'frontend-design', name: 'Design', receipt: recibo('frontend-design', D1), contentStatus: 'changed' })
  const hub = hubFalso({ itens: [item({ installed: true, installedId: 'frontend-design' })], instaladas: [curada] })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  await loja.reinstalar(inst(9, { id: 'manual', name: 'Manual', contentStatus: 'changed' }), loja.capturarIntencao())
  assert.equal(hub.chamadas.some(([m]) => m === 'installSkill'), false, 'manual não tem reinstalação')
  await loja.reinstalar(loja.instaladas![0]!, loja.capturarIntencao())
  assert.deepEqual(hub.chamadas.filter(([m]) => m === 'installSkill').map(([, c]) => c),
    [{ skillProtocol: 2, catalogId: 'frontend-design', expectedDigest: D1, expectedInstallationId: `${NS}:3` }])
  assert.equal(pt(loja.aviso), pt(textos.reinstalada('Design')))
  assert.equal(loja.instaladas![0]!.contentStatus, 'verified')
})

test('detalhes: pedido forte preso ao Hub da folha; resposta depois de trocar de Hub não aparece', async () => {
  const instalada = inst(1, { id: 'nota', name: 'Nota', source: 'custom' })
  let soltar!: () => void
  const chamadas: unknown[] = []
  const a: SkillClient = { async call<T>(metodo: string, corpo?: unknown): Promise<T> {
    if (metodo === 'skills') return { skillProtocol: 2, items: [instalada] } as T
    if (metodo === 'marketSkills') return { skillProtocol: 2, items: [] } as T
    chamadas.push(corpo)
    await new Promise<void>((r) => { soltar = r })
    return { skillProtocol: 2, markdown: 'TEXTO A', catalog: null, receipt: null, installation: instalada } as T
  } }
  let vivo = { hub: 'hub-a', client: a as SkillClient | null, geracao: 1 }
  const loja = new CatalogoStore(async () => a, () => {}, () => vivo)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const origem = loja.capturarIntencao()
  const pedido = loja.detalhes({ id: 'nota' }, origem)
  await esperar()
  assert.deepEqual(chamadas, [{ id: 'nota', skillProtocol: 2 }])
  vivo = { hub: 'hub-b', client: null, geracao: 2 }
  soltar()
  assert.equal(await pedido, null, 'resposta de A não aparece depois da troca')
  vivo = { hub: 'hub-a', client: a, geracao: 3 }
  const deNovo = loja.detalhes({ id: 'nota' }, origem)
  assert.equal(await deNovo, null, 'reconexão não revive a origem antiga')
  assert.equal(chamadas.length, 1, 'nenhum pedido novo com origem antiga')
})

// R8: resultado incerto depois do envio — uma chamada, sem repetir, releitura do mesmo Hub.

/** Envolve o Hub falso: a mutação faz efeito e a resposta se perde (erro) ou chega malformada. */
function perderResposta(hub: ReturnType<typeof hubFalso>, metodo: string, como: Error | unknown) {
  const original = hub.client.call.bind(hub.client)
  hub.client.call = async <T,>(m: string, corpo?: unknown): Promise<T> => {
    const r = await original<T>(m, corpo)
    if (m !== metodo) return r
    if (como instanceof Error) throw como
    return como as T
  }
}

test('incerto ao instalar: uma chamada, sem anúncio de sucesso; relê o mesmo Hub e diz o que a lista mostra agora', async () => {
  const hub = hubFalso({ itens: [item()], instaladas: [] })
  perderResposta(hub, 'installSkill', new Error('A conexão caiu antes da resposta.'))
  let mudou = 0
  const loja = new CatalogoStore(async () => hub.client, () => { mudou++ })
  loja.sincronizar('hub-a')
  await loja.carregar()
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.equal(hub.chamadas.filter(([m]) => m === 'installSkill').length, 1, 'nenhuma repetição')
  const incerto = loja.incertos.get('frontend-design')!
  assert.equal(incerto.metodo, 'installSkill')
  assert.equal(incerto.conferido, true, 'a releitura automática do mesmo Hub conferiu')
  assert.equal(pt(incerto.mensagem), `${pt(textos.incerto.instalar('Design de interfaces'))} Resposta do Hub: A conexão caiu antes da resposta. ${pt(textos.incertoAgora.instalada('Design de interfaces'))}`)
  assert.equal(loja.falhas.has('frontend-design'), false)
  assert.equal(loja.instaladaAgora, null, 'nada de "instalada" por inferência')
  assert.equal(pt(loja.aviso), pt(incerto.mensagem))
  assert.equal(mudou, 1, 'os ajustes de bot releem (só leitura)')
  assert.equal(loja.pedidos.size, 0)
})

test('incerto sem conferência bloqueia gesto novo na chave; a releitura bem-sucedida libera', async () => {
  const hub = hubFalso({ itens: [item(), item({ id: 'outra', name: 'Outra' })], instaladas: [] })
  perderResposta(hub, 'installSkill', { skillProtocol: 1 })
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  // A releitura automática falha: o incerto fica sem conferir.
  const original = hub.client.call.bind(hub.client)
  let lerFalha = true
  hub.client.call = async <T,>(m: string, corpo?: unknown): Promise<T> => {
    if (lerFalha && (m === 'skills' || m === 'marketSkills') && hub.chamadas.some(([x]) => x === 'installSkill')) throw new Error('Hub ocupado')
    return original<T>(m, corpo)
  }
  await loja.instalar(loja.itens![1]!, loja.capturarIntencao())
  assert.equal(loja.aguardandoConferir('outra'), true)
  assert.equal(pt(loja.incertos.get('outra')!.mensagem), pt(textos.incerto.instalar('Outra')), 'resposta malformada não vira texto do Hub')
  await loja.instalar(loja.itens![1]!, loja.capturarIntencao())
  assert.equal(hub.chamadas.filter(([m]) => m === 'installSkill').length, 1, 'nenhum gesto novo antes de conferir')
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.equal(hub.chamadas.filter(([m]) => m === 'installSkill').length, 2, 'outras chaves continuam livres')
  lerFalha = false
  await loja.carregar()
  assert.equal(loja.aguardandoConferir('outra'), false)
  assert.match(pt(loja.incertos.get('outra')!.mensagem)!, /A lista de agora mostra Outra instalada\./)
})

test('incerto ao remover: aviso preso à encarnação pedida; ausência é descrita, não atribuída', async () => {
  const hub = hubFalso({ itens: [], instaladas: [inst(1, { id: 'nota', name: 'Nota', source: 'custom' })] })
  perderResposta(hub, 'removeSkill', undefined)
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  await loja.remover(alvo(loja.instaladas![0]!), loja.capturarIntencao())
  assert.equal(hub.chamadas.filter(([m]) => m === 'removeSkill').length, 1)
  const incerto = loja.incertos.get('rm:nota')!
  assert.equal(incerto.alvo, `${NS}:1`)
  assert.equal(pt(incerto.mensagem), `${pt(textos.incerto.remover('Nota'))} ${pt(textos.incertoAgora.ausente('Nota'))}`)
  assert.doesNotMatch(pt(loja.aviso) ?? '', /removida/)
})

test('incerto ao escrever: rascunho volta como incerto, sem concluir pela skill de mesmo nome na lista', async () => {
  const hub = hubFalso({ itens: [], instaladas: [inst(1, { id: 'nota', name: 'Nota', source: 'custom' })] })
  const original = hub.client.call.bind(hub.client)
  hub.client.call = async <T,>(m: string, corpo?: unknown): Promise<T> => {
    if (m !== 'installSkill') return original<T>(m, corpo)
    hub.chamadas.push([m, corpo])
    // O Hub grava uma segunda manual idêntica e a resposta chega sem a prova de versão.
    hub.estado.instaladas.push(inst(2, { id: 'nota-2', name: 'Nota', source: 'custom' }))
    return { skill: hub.estado.instaladas.at(-1), alreadyInstalled: false } as T
  }
  const loja = new CatalogoStore(async () => hub.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const rascunho = { name: 'Nota', description: '', instructions: 'Texto.' }
  assert.equal(await loja.escrever(rascunho, loja.capturarIntencao()), 'incerto')
  assert.equal(hub.chamadas.filter(([m]) => m === 'installSkill').length, 1, 'nada reenviado')
  assert.deepEqual(hub.chamadas.find(([m]) => m === 'installSkill')![1], { skillProtocol: 2, ...rascunho })
  assert.equal(pt(loja.incertos.get('escrever')!.mensagem), pt(textos.incerto.escrever('Nota')), 'sem descrição por nome ou texto')
  assert.doesNotMatch(pt(loja.aviso) ?? '', /adicionada/)
})

test('b2 regressão: geração nova no mesmo client durante ready não envia detalhes antigos', async () => {
  const hub = hubFalso({ itens: [item()], instaladas: [] })
  const call = hub.client.call.bind(hub.client)
  hub.client.call = async <T,>(m: string, b?: unknown): Promise<T> => {
    if (m !== 'skillDetails') return call<T>(m, b)
    hub.chamadas.push([m, b])
    return { skillProtocol: 2, markdown: 'TEXTO ANTIGO', catalog: item(), receipt: null, installation: null } as T
  }
  let geracao = 1
  let obter = async () => hub.client
  const loja = new CatalogoStore(() => obter(), () => {}, () => ({ hub: 'hub-a', client: hub.client, geracao }))
  loja.sincronizar('hub-a')
  await loja.carregar()
  const origem = loja.capturarIntencao()
  let liberar!: (c: SkillClient) => void
  obter = () => new Promise((r) => { liberar = r })
  const pedido = loja.detalhes({ source: 'frontend-design' }, origem)
  geracao++
  liberar(hub.client)
  assert.equal(await pedido, null)
  assert.equal(hub.chamadas.filter(([m]) => m === 'skillDetails').length, 0)
})

for (const operacao of ['listas', 'detalhes'] as const) {
  test(`b2 ${operacao}: sessão/origem conferidas antes de ready, após ready e resposta`, async () => {
    for (const fase of ['entrada', 'ready', 'resposta'] as const) {
      const chamadas: string[] = []
      let liberarReady!: () => void
      let liberarResposta!: () => void
      let valida = true
      let geracao = 1
      let prontidoes = 0
      const esperaReady = new Promise<void>((r) => { liberarReady = r })
      const esperaResposta = new Promise<void>((r) => { liberarResposta = r })
      const client: SkillClient = { async call<T>(m: string): Promise<T> {
        chamadas.push(m)
        await esperaResposta
        return (m === 'skillDetails'
          ? { skillProtocol: 2, markdown: 'CANÔNICO', catalog: item(), receipt: null, installation: null }
          : { skillProtocol: 2, items: m === 'marketSkills' ? [item()] : [] }) as T
      } }
      const loja = new CatalogoStore(async () => { prontidoes++; await esperaReady; return client }, () => {}, () => ({ hub: 'hub-a', client, geracao }))
      loja.sincronizar('hub-a', { adiarCarga: true })
      const origem = loja.capturarIntencao()!
      const escopo = { origem, valida: () => valida }
      if (fase === 'entrada') valida = false
      const pedido = operacao === 'listas' ? loja.carregar(escopo) : loja.detalhes({ source: item().id }, origem, escopo)
      await esperar()
      if (fase === 'ready') geracao += 2 // A→B→A no mesmo client, sem render.
      liberarReady()
      await esperar()
      if (fase === 'resposta') valida = false // sessão fechada antes da resposta.
      liberarResposta()
      const resultado = await pedido
      assert.equal(prontidoes, fase === 'entrada' ? 0 : 1)
      assert.equal(chamadas.length, fase === 'resposta' ? (operacao === 'listas' ? 2 : 1) : 0)
      if (operacao === 'detalhes') assert.equal(resultado, null)
      assert.equal(loja.itens, null, 'resposta antiga nunca publica lista')
      assert.equal(loja.instaladas, null)
      assert.equal(loja.erro, null)
      assert.equal(loja.carregando, false)
      assert.equal(loja.listasDaOrigem(origem), false)
      assert.ok(chamadas.every((m) => ['marketSkills', 'skills', 'skillDetails'].includes(m)), 'zero installSkill/removeSkill/updateBot')
    }
  })
}

test('b2 listas: cache de geração anterior e cliente errado de ready não viram resultado válido', async () => {
  const hub = hubFalso({ itens: [item()], instaladas: [] })
  const outro = hubFalso({ itens: [item()], instaladas: [] })
  let geracao = 1
  let pronto = hub.client
  const loja = new CatalogoStore(async () => pronto, () => {}, () => ({ hub: 'hub-a', client: hub.client, geracao }))
  loja.sincronizar('hub-a')
  await loja.carregar()
  assert.equal(loja.listasDaOrigem(loja.capturarIntencao()!), true)
  geracao++
  const origem = loja.capturarIntencao()!
  assert.equal(loja.listasDaOrigem(origem), false)
  pronto = outro.client
  await loja.carregar({ origem, valida: () => true })
  assert.deepEqual(outro.chamadas, [])
  assert.match(pt(loja.erro)!, /conexão.*mudou/)
  assert.equal(loja.listasDaOrigem(origem), false)
})

test('b2 detalhes: falha de ready de sessão fechada também é descartada', async () => {
  const hub = hubFalso({ itens: [], instaladas: [] })
  let falhar!: (e: Error) => void
  let valida = true
  const loja = new CatalogoStore(() => new Promise((_, r) => { falhar = r }), () => {}, () => ({ hub: 'hub-a', client: hub.client, geracao: 1 }))
  loja.sincronizar('hub-a', { adiarCarga: true })
  const origem = loja.capturarIntencao()!
  const pedido = loja.detalhes({ source: item().id }, origem, { origem, valida: () => valida })
  valida = false
  falhar(new Error('falha tardia de ready'))
  assert.equal(await pedido, null)
  assert.deepEqual(hub.chamadas, [])
})
