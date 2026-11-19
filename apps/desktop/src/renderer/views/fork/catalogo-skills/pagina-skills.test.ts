import assert from 'node:assert/strict'
import test from 'node:test'
import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { CatalogSkill, InstalledSkillV2, SkillDetailsV2, SkillReceipt } from '../../../store/fork-skills.ts'
import { CatalogoStore } from './catalogo-store.ts'
import { agruparInstaladas, alvoDaAba, capturarRemocao, detalhesDaVersao, fichaDaInstalada, filtrarInstaladas, mesmaInstalacao, versaoDaFolha } from './regras.ts'
import { textoDe, type Frase } from '../../../lib/fork-i18n/frase.ts'

// D454 (fatia 8b): a loja guarda frases; o texto pt-BR, sem os isolantes bidi dos nomes, é o de antes.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : '')


// D70: só o que a página Skills mudou — busca e grupos das instaladas, teclas das abas, a
// fronteira de Hub e de conexão da skill escrita à mão e das intenções, e a ficha da instalada.

const D1 = 'd1'.repeat(32)
const NS = '22222222-2222-4222-8222-222222222222'
let sequencia = 0
const recibo = (catalogId: string): SkillReceipt => ({
  schema: 1, catalogId, digest: D1, origin: { repo: 'org/skills', revision: 'a'.repeat(40), path: `skills/${catalogId}`, sourceDigest: 'b'.repeat(64) },
  adaptation: null, license: 'MIT', files: [], result: 'imported', coverage: 'pinnedPassiveTextOnly', limitations: [],
})
/** Uma instalada como o Hub R7 manda; cada chamada é uma encarnação nova. */
const instalada = (id: string, name: string, description: string, comRecibo: boolean): InstalledSkillV2 => ({
  id, name, description, source: '', path: `skills/${id}`, receipt: comRecibo ? recibo(id) : null, installationId: `${NS}:${++sequencia}`, contentStatus: 'verified',
})

test('instaladas: busca sem acento por nome, descrição e id, na ordem do Hub e sem reescrever o texto', () => {
  const lista = [
    instalada('find-skills', 'find-skills', 'Helps users discover and install agent skills.', true),
    instalada('manual', 'Revisão de contratos', 'Lê cláusulas e aponta riscos.', false),
    instalada('frontend-design', 'frontend-design', 'Guidance for distinctive, intentional visual design.', true),
  ]
  assert.deepEqual(filtrarInstaladas(lista, 'DESIGN').map((s) => s.id), ['frontend-design'])
  assert.deepEqual(filtrarInstaladas(lista, 'clausulas').map((s) => s.id), ['manual'])
  assert.deepEqual(filtrarInstaladas(lista, 'find-').map((s) => s.id), ['find-skills'])
  assert.deepEqual(filtrarInstaladas(lista, '  ').map((s) => s.id), ['find-skills', 'manual', 'frontend-design'])
  assert.equal(filtrarInstaladas(lista, 'helps')[0], lista[0], 'o mesmo objeto do Hub, sem tradução nem cópia alterada')
})

test('instaladas: grupos só por recibo, nunca por nome; grupo vazio sai', () => {
  // Uma antiga sem recibo com o mesmo nome de uma do catálogo continua sem recibo.
  const lista = [
    instalada('pdf', 'pdf', 'Legacy text.', false),
    instalada('pdf-donna', 'pdf', 'Catalog text.', true),
  ]
  const grupos = agruparInstaladas(lista)
  assert.deepEqual(grupos.map((g) => [g.id, g.skills.map((s) => s.id)]), [['catalogo', ['pdf-donna']], ['semRecibo', ['pdf']]])
  assert.deepEqual(agruparInstaladas([lista[1]!]).map((g) => g.id), ['catalogo'])
  assert.deepEqual(agruparInstaladas([]), [])
})

test('abas: ←/→ voltam ao outro lado, Home/End vão às pontas, modificadores ficam com a janela', () => {
  const tecla = (key: string, extra: Partial<{ altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {}) => ({ key, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...extra })
  assert.equal(alvoDaAba(tecla('ArrowRight'), 1, 2), 0)
  assert.equal(alvoDaAba(tecla('ArrowLeft'), 0, 2), 1)
  assert.equal(alvoDaAba(tecla('Home'), 1, 2), 0)
  assert.equal(alvoDaAba(tecla('End'), 0, 2), 1)
  assert.equal(alvoDaAba(tecla('ArrowDown'), 0, 2), null)
  assert.equal(alvoDaAba(tecla('ArrowRight', { metaKey: true }), 0, 2), null)
})

/** Um Hub que só lê e grava skills à mão, com envelopes do protocolo forte; registra cada chamada. */
function hubManual(nome: string) {
  const chamadas: [string, unknown][] = []
  const instaladas: InstalledSkillV2[] = []
  let falhar: string | null = null
  const client: SkillClient = {
    async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      chamadas.push([metodo, corpo])
      if (metodo === 'marketSkills') return { skillProtocol: 2, items: [] } as T
      if (metodo === 'skills') return { skillProtocol: 2, items: structuredClone(instaladas) } as T
      if (metodo === 'installSkill') {
        if (falhar) throw new Error(falhar)
        const { name, description } = corpo as { name: string; description: string }
        const skill = instalada(`${nome}-${name}`, name, description, false)
        instaladas.push(skill)
        return { skillProtocol: 2, skill, alreadyInstalled: false } as T
      }
      throw new Error(`método inesperado: ${metodo}`)
    },
  }
  return { client, chamadas, instaladas, falhar: (motivo: string | null) => { falhar = motivo } }
}
const gravacoes = (h: ReturnType<typeof hubManual>) => h.chamadas.filter(([m]) => m === 'installSkill')

test('escrever à mão: vai ao Hub da origem como foi escrito e só conclui depois de reler', async () => {
  const a = hubManual('a')
  let mudou = 0
  const loja = new CatalogoStore(async () => a.client, () => { mudou++ })
  loja.sincronizar('hub-a')
  await loja.carregar()
  const rascunho = { name: 'Revisão de contratos', description: 'Quando houver um contrato.', instructions: '1. Leia tudo.\n2. Aponte riscos.' }
  const [primeiro, duplo] = await Promise.all([loja.escrever(rascunho, loja.capturarIntencao()), loja.escrever(rascunho, loja.capturarIntencao())])
  assert.equal(primeiro, 'apareceu')
  assert.equal(duplo, 'falhou', 'clique duplo não grava duas vezes')
  assert.deepEqual(gravacoes(a), [['installSkill', { skillProtocol: 2, ...rascunho }]], 'o texto vai como a pessoa escreveu, sem campos de catálogo')
  assert.deepEqual(loja.instaladas!.map((s) => s.name), ['Revisão de contratos'], 'a lista relida mostra a skill')
  assert.equal(mudou, 1, 'os ajustes de bot releem as skills; nada é ligado em bot')

  // R7: recusa sem código conhecido depois do envio é incerta; o motivo do Hub aparece como dado.
  a.falhar('invalid_skill: instruções grandes demais')
  assert.equal(await loja.escrever({ ...rascunho, name: 'Outra' }, loja.capturarIntencao()), 'incerto')
  assert.match(pt(loja.incertos.get('escrever')!.mensagem), /Resposta do Hub: instruções grandes demais/, 'o motivo fica para a folha manter o rascunho')
  assert.equal(loja.pedidos.size, 0)
})

test('escrever à mão: origem de A não grava em B nem em A de novo numa época nova', async () => {
  const a = hubManual('a')
  const b = hubManual('b')
  let atual = a
  const loja = new CatalogoStore(async () => atual.client)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const origemA = loja.capturarIntencao()
  const rascunho = { name: 'Nota', description: '', instructions: 'Texto.' }
  atual = b
  loja.sincronizar('hub-b')
  await loja.carregar()
  assert.equal(await loja.escrever(rascunho, origemA), 'falhou')
  atual = a
  loja.sincronizar('hub-a')
  await loja.carregar()
  assert.equal(await loja.escrever(rascunho, origemA), 'falhou', 'mesmo hostId não revive a folha aberta antes')
  assert.equal(await loja.escrever(rascunho, null), 'falhou', 'sem origem não grava')
  assert.deepEqual([...gravacoes(a), ...gravacoes(b)], [])
  assert.equal(loja.aviso, null)
  assert.equal(await loja.escrever(rascunho, loja.capturarIntencao()), 'apareceu', 'folha aberta agora grava no Hub atual')
  assert.equal(gravacoes(a).length, 1)
})

// R2 (revisão Sol 1): F2 — a intenção leva a conexão de origem; F3 — detalhes da instalada.

/** Um Hub com várias conexões (clientes) ao longo do tempo; registra cada chamada por cliente. */
function hubConexoes() {
  const chamadas: { cliente: string; metodo: string; corpo: unknown }[] = []
  const instaladas: InstalledSkillV2[] = [instalada('manual', 'Manual', 'Texto.', false)]
  const itens = [{
    id: 'alpha', source: 'alpha', name: 'alpha', description: 'Original description.', category: 'Design', aliases: [],
    origin: { repo: 'org/skills', revision: 'a'.repeat(40), path: 'skills/alpha', sourceDigest: 'b'.repeat(64) },
    license: 'MIT', availability: 'text' as const, reason: '', requirements: [], losses: [], adaptation: null, digest: D1,
    files: [], installable: true, installed: false, installedId: null, readiness: null,
  }]
  const cliente = (nome: string): SkillClient => ({
    async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      chamadas.push({ cliente: nome, metodo, corpo })
      if (metodo === 'marketSkills') return { skillProtocol: 2, items: structuredClone(itens) } as T
      if (metodo === 'skills') return { skillProtocol: 2, items: structuredClone(instaladas) } as T
      if (metodo === 'removeSkill') return { skillProtocol: 2 } as T
      return { skillProtocol: 2, skill: instalada('nova', 'Nova', '', false), alreadyInstalled: false } as T
    },
  })
  return { cliente, chamadas, mutacoes: () => chamadas.filter((c) => c.metodo === 'installSkill' || c.metodo === 'removeSkill') }
}

test('conexão: A→B→A sem sincronizar não revive o gesto antigo; um gesto novo vai pela conexão de agora', async () => {
  const h = hubConexoes()
  const c1 = h.cliente('c1')
  let vivo = { hub: 'hub-a' as string | null, client: c1 as SkillClient | null, geracao: 1 }
  const loja = new CatalogoStore(async () => vivo.client!, () => {}, () => vivo)
  loja.sincronizar('hub-a')
  await loja.carregar()
  const origem = loja.capturarIntencao()
  // Troca e volta no mesmo tique: a tela não renderizou, a loja não sincronizou, a época é a mesma.
  vivo = { hub: 'hub-b', client: h.cliente('c2'), geracao: 2 }
  const c3 = h.cliente('c3')
  vivo = { hub: 'hub-a', client: c3, geracao: 3 }
  assert.equal(loja.intencaoAtual(origem), true, 'Hub e época ainda batem')
  assert.equal(loja.conexaoAtual(origem), false, 'a conexão não')
  await loja.instalar(loja.itens![0]!, origem)
  await loja.remover(capturarRemocao(loja.instaladas![0]!)!, origem)
  assert.equal(await loja.escrever({ name: 'Nota', description: '', instructions: 'Texto.' }, origem), 'falhou')
  assert.deepEqual(h.mutacoes(), [], 'nenhum RPC por nenhuma conexão')
  assert.equal(loja.pedidos.size, 0)
  assert.equal(loja.aviso, null)
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.deepEqual(h.mutacoes().map((c) => [c.cliente, c.metodo]), [['c3', 'installSkill']], 'gesto novo funciona no Hub atual')
})

test('conexão: reconexão no mesmo cliente, troca durante ready e gesto sem conexão não enviam nada e não prendem o botão', async () => {
  const h = hubConexoes()
  const c1 = h.cliente('c1')
  let vivo = { hub: 'hub-a' as string | null, client: c1 as SkillClient | null, geracao: 1 }
  let soltar: (() => void) | null = null
  let entregue: SkillClient = c1
  const loja = new CatalogoStore(async () => {
    if (soltar === null) return vivo.client!
    await new Promise<void>((r) => { soltar = () => r() })
    return entregue
  }, () => {}, () => vivo)
  loja.sincronizar('hub-a')
  await loja.carregar()

  // Mesmo objeto de cliente, outra geração do link (novo hello do stream).
  const antes = loja.capturarIntencao()
  vivo = { ...vivo, geracao: 2 }
  await loja.instalar(loja.itens![0]!, antes)
  assert.deepEqual(h.mutacoes(), [])

  // A conexão muda enquanto ready() espera: o cliente entregue é de outra conexão, mesmo hostId.
  const agora = loja.capturarIntencao()
  soltar = () => {}
  const pedido = loja.instalar(loja.itens![0]!, agora)
  await new Promise((r) => setTimeout(r, 0))
  assert.equal(loja.pedido('alpha')?.fase, 'enviando')
  const c3 = h.cliente('c3')
  vivo = { hub: 'hub-a', client: c3, geracao: 3 }
  entregue = c3
  const liberar = soltar as () => void
  soltar = null
  liberar()
  await pedido
  assert.deepEqual(h.mutacoes(), [], 'nem pela conexão antiga nem pela nova')
  assert.equal(loja.pedidos.size, 0, 'o botão não fica preso')
  assert.match(pt(loja.falhas.get('alpha')), /conexão com o Hub mudou/)

  // Gesto feito sem link: avisa, sem esperar uma reconexão que pode ser de outro Hub.
  vivo = { hub: 'hub-a', client: null, geracao: 3 }
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.deepEqual(h.mutacoes(), [])
  assert.match(pt(loja.falhas.get('alpha')), /Sem conexão com o Hub agora/)
  assert.equal(loja.pedidos.size, 0)
  vivo = { hub: 'hub-a', client: c3, geracao: 4 }
  await loja.instalar(loja.itens![0]!, loja.capturarIntencao())
  assert.deepEqual(h.mutacoes().map((c) => c.cliente), ['c3'], 'com o link de volta, o gesto novo instala')
})

test('instalar sem origem explícita não captura a conexão de agora', async () => {
  const h = hubConexoes()
  const c1 = h.cliente('c1')
  const loja = new CatalogoStore(async () => c1, () => {}, () => ({ hub: 'hub-a', client: c1, geracao: 1 }))
  loja.sincronizar('hub-a')
  await loja.carregar()
  await (loja.instalar as (i: unknown) => Promise<void>)(loja.itens![0]!)
  assert.deepEqual(h.mutacoes(), [])
  assert.equal(loja.pedidos.size, 0)
})

test('detalhes da instalada: registro e recibo dela; capturado só da mesma versão; versão atual à parte', () => {
  const origem = (repo: string, rev: string) => ({ repo, revision: rev.repeat(40), path: `skills/${rev}`, sourceDigest: rev.repeat(64) })
  const versao = (digest: string, patch: Partial<CatalogSkill> = {}): CatalogSkill => ({
    id: 'legacy', source: 'legacy', name: 'Original source title v2', description: 'Current description.', category: 'Bases', aliases: [],
    origin: origem('org/source', 'd'), license: 'MIT', availability: 'text', reason: '', requirements: [], losses: [], adaptation: null,
    digest, files: [{ path: 'SKILL.md', bytes: 10, sha256: 'd'.repeat(64), mode: '100644' }], installable: true, installed: false, installedId: null, readiness: null, ...patch,
  })
  const reciboAntigo: SkillReceipt = {
    schema: 1, catalogId: 'legacy', digest: '9'.repeat(64), origin: origem('org/old-source', '9'), adaptation: null, license: 'Apache-2.0',
    files: [{ path: 'SKILL.md', bytes: 7, sha256: '9'.repeat(64), mode: '100644' }], result: 'imported', coverage: 'pinnedPassiveTextOnly', limitations: [],
  }
  const legado: InstalledSkillV2 = { id: 'legacy-id', name: 'Nome legado traduzido', description: 'Descrição antiga.', source: '', path: '', receipt: reciboAntigo, installationId: `${NS}:900`, contentStatus: 'verified' }
  const atual = versao('d'.repeat(64))
  const capturado = versao('9'.repeat(64), { name: 'Nome legado traduzido', origin: reciboAntigo.origin, license: 'Apache-2.0', installable: false, installed: true, installedId: 'legacy-id', losses: ['antiga'] })

  const ficha = fichaDaInstalada(legado, capturado, atual)
  assert.equal(ficha.nome, 'Nome legado traduzido', 'o nome gravado, sem tradução nem reescrita')
  assert.equal(ficha.descricao, 'Descrição antiga.')
  assert.equal(ficha.recibo, reciboAntigo)
  assert.equal(ficha.capturado, capturado, 'metadados capturados da versão do recibo')
  assert.equal(ficha.outraVersao, atual, 'a versão de agora só como outra versão')

  // Hub que devolve o catálogo atual (ou outro envelope) junto do recibo antigo: não vale.
  assert.equal(fichaDaInstalada(legado, atual, atual).capturado, null)
  // Recibo desconhecido (catalog: null): só o recibo, sem origem de outra versão.
  const desconhecido = fichaDaInstalada(legado, null, null)
  assert.equal(desconhecido.capturado, null)
  assert.equal(desconhecido.outraVersao, null)
  assert.equal(desconhecido.recibo?.origin.repo, 'org/old-source')
  // Mesma versão no catálogo: sem cartão extra.
  assert.equal(fichaDaInstalada({ ...legado, receipt: { ...reciboAntigo, digest: atual.digest } }, null, atual).outraVersao, null)
  // Sem recibo: nada do catálogo, nem com o mesmo nome.
  const manual: InstalledSkillV2 = { ...legado, id: 'manual', name: atual.name, receipt: null }
  const semRecibo = fichaDaInstalada(manual, atual, atual)
  assert.deepEqual([semRecibo.recibo, semRecibo.capturado, semRecibo.outraVersao], [null, null, null])
})

// R5 (revisão Sol 4, F4): uma versão por folha e confirmação presa à instalação vista; R8: encarnação.

/** A mesma ID antes e depois de outro cliente trocar a manual pelo pacote do catálogo. */
const manualNaId: InstalledSkillV2 = { id: 'frontend-design', name: 'frontend-design', description: 'Synthetic manual predecessor', source: 'custom', path: '/tmp/fixture/frontend-design/SKILL.md', receipt: null, installationId: `${NS}:501`, contentStatus: 'verified' }
const catalogoNaId: InstalledSkillV2 = { ...manualNaId, description: 'Original English description.', source: 'catalog:frontend-design', receipt: { ...recibo('frontend-design'), digest: 'e'.repeat(64) }, installationId: `${NS}:502` }
/** A mesma manual escrita de novo por outro cliente: registro idêntico, outra encarnação. */
const manualDeNovo: InstalledSkillV2 = { ...manualNaId, installationId: `${NS}:503` }
const detalhe = (markdown: string, installation: InstalledSkillV2 | null, catalog: CatalogSkill | null = null): SkillDetailsV2 =>
  ({ skillProtocol: 2, markdown, catalog, receipt: installation?.receipt ?? null, installation })

test('uma versão por folha: outra encarnação ou outro registro na mesma ID é outra versão; a resposta precisa ser da versão mostrada', () => {
  assert.notEqual(versaoDaFolha(null, manualNaId), versaoDaFolha(null, catalogoNaId), 'manual → catálogo na mesma ID')
  assert.equal(versaoDaFolha(null, structuredClone(manualNaId)), versaoDaFolha(null, manualNaId), 'lista relida com a mesma encarnação: mesma versão')
  assert.equal(mesmaInstalacao(manualNaId, { ...manualNaId }), true)
  assert.equal(mesmaInstalacao(manualNaId, { ...manualNaId, description: 'Outra descrição.' }), false, 'manual reescrita com outra descrição')
  assert.notEqual(versaoDaFolha(null, manualDeNovo), versaoDaFolha(null, manualNaId), 'reinstalação idêntica é outra versão (R8)')
  assert.equal(mesmaInstalacao(manualNaId, manualDeNovo), false)
  assert.equal(mesmaInstalacao(manualNaId, { ...manualNaId, contentStatus: 'changed' }), false, 'conteúdo que deixou de conferir')

  const antiga = detalhe('OLD_MANUAL_BODY', manualNaId)
  const nova = detalhe('NEW_CATALOG_BODY', catalogoNaId)
  assert.equal(detalhesDaVersao(antiga, null, manualNaId), true)
  assert.equal(detalhesDaVersao(nova, null, catalogoNaId), true)
  assert.equal(detalhesDaVersao(antiga, null, catalogoNaId), false, 'texto da manual não fica sob o recibo novo')
  assert.equal(detalhesDaVersao(nova, null, manualNaId), false, 'texto do pacote não fica sob o registro manual (lista atrasada)')
  assert.equal(detalhesDaVersao(detalhe('NEW_MANUAL_BODY', manualDeNovo), null, manualNaId), false, 'manual idêntica reinstalada não empresta o texto à linha antiga')
  assert.equal(detalhesDaVersao(detalhe('x', null), null, manualNaId), false, 'detalhe sem instalação não vale para uma instalada')

  const item: CatalogSkill = {
    id: 'frontend-design', source: 'frontend-design', name: 'frontend-design', description: 'Original English description.', category: 'Design', aliases: [],
    origin: { repo: 'org/skills', revision: 'a'.repeat(40), path: 'skills/frontend-design', sourceDigest: 'b'.repeat(64) },
    license: 'MIT', availability: 'text', reason: '', requirements: [], losses: [], adaptation: null, digest: 'e'.repeat(64),
    files: [], installable: false, installed: false, installedId: null, readiness: null,
  }
  assert.equal(versaoDaFolha({ ...item, installed: true, installedId: 'frontend-design', installable: true, reason: 'outro motivo' }, null), versaoDaFolha(item, null), 'só estado: mesma versão do catálogo')
  assert.notEqual(versaoDaFolha({ ...item, digest: 'f'.repeat(64) }, null), versaoDaFolha(item, null))
  assert.equal(detalhesDaVersao(detalhe('x', null, item), item, null), true)
  assert.equal(detalhesDaVersao(detalhe('x', null, { ...item, digest: 'f'.repeat(64) }), item, null), false)
  assert.equal(detalhesDaVersao(detalhe('x', null, null), item, null), false)
  assert.equal(detalhesDaVersao(detalhe('x', catalogoNaId, item), item, null), false, 'catálogo com instalação não é a ficha do catálogo')
})

test('remover: a confirmação da manual não remove o pacote instalado depois na mesma ID; a mesma instalação remove', async () => {
  let lista: InstalledSkillV2[] = [manualNaId]
  const remocoes: unknown[] = []
  let duranteReady: (() => Promise<void>) | null = null
  const client: SkillClient = {
    async call<T>(metodo: string, corpo?: unknown): Promise<T> {
      if (metodo === 'marketSkills') return { skillProtocol: 2, items: [] } as T
      if (metodo === 'skills') return { skillProtocol: 2, items: structuredClone(lista) } as T
      if (metodo === 'removeSkill') {
        // Como o Host R7: remove só a encarnação pedida.
        remocoes.push(corpo)
        const pedido = corpo as { id: string; expectedInstallationId: string }
        if (lista.some((s) => s.id === pedido.id && s.installationId !== pedido.expectedInstallationId)) throw new Error('skill_installation_stale')
        lista = lista.filter((s) => s.id !== pedido.id)
        return { skillProtocol: 2 } as T
      }
      throw new Error(`método inesperado: ${metodo}`)
    },
  }
  const loja = new CatalogoStore(async () => {
    const antes = duranteReady
    duranteReady = null
    if (antes) await antes()
    return client
  }, () => {}, () => ({ hub: 'hub-a', client, geracao: 1 }))
  loja.sincronizar('hub-a')
  await loja.carregar()

  // Confirmação aberta na manual; outro cliente troca pelo pacote; a página relê.
  const capturada = capturarRemocao(loja.instaladas![0]!)!
  const origem = loja.capturarIntencao()
  lista = [catalogoNaId]
  await loja.carregar()
  await loja.remover(capturada, origem)
  assert.deepEqual(remocoes, [], 'nenhum removeSkill pela ID que agora é de outra instalação')
  assert.match(pt(loja.falhas.get('rm:frontend-design')), /instalação mudou desde que você pediu para remover/)
  assert.equal(loja.pedidos.size, 0)
  assert.deepEqual(loja.instaladas!.map((s) => s.source), ['catalog:frontend-design'], 'a versão nova continua')

  // A troca chega enquanto ready() espera: a conferência logo antes do RPC também barra.
  lista = [manualNaId]
  await loja.carregar()
  const deNovo = capturarRemocao(loja.instaladas![0]!)!
  duranteReady = async () => { lista = [catalogoNaId]; await loja.carregar() }
  await loja.remover(deNovo, loja.capturarIntencao())
  assert.deepEqual(remocoes, [])
  assert.equal(loja.pedidos.size, 0, 'o botão não fica preso')

  // Contraprova: a instalação que a pessoa viu continua lá, então remove a encarnação capturada.
  lista = [manualNaId]
  await loja.carregar()
  await loja.remover(capturarRemocao(loja.instaladas![0]!)!, loja.capturarIntencao())
  assert.deepEqual(remocoes, [{ skillProtocol: 2, id: 'frontend-design', expectedInstallationId: manualNaId.installationId, expectedReceipt: { kind: 'none' } }])
})
