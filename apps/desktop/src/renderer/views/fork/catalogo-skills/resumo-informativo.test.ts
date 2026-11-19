import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import type { InstalledSkillV2, SkillClient, SkillDetailsV2, SkillFile } from '../../../../shared/fork-skills.ts'
import { catalogoFor, type CatalogoStore } from './catalogo-store.ts'
import { detalhesDaVersao, versaoDaFolha } from './regras.ts'
import { estadoDaConferencia, leituraVigente, manifestoApresentavel, resumoDaInstalada, type EstadoConferencia, type SinaisDaFolha } from './resumo-informativo.ts'

// Quatro fronteiras D70; transporte, recibos e caminhos são sentinelas sintéticas.
const NS = '11111111-1111-4111-8111-111111111111'
const arquivo = (path = 'SKILL.md', bytes = 40): SkillFile => ({ path, bytes, sha256: 'a'.repeat(64), mode: '100644' })
const manual = (): InstalledSkillV2 => ({ id: 'notas', name: 'Notas', description: 'Sintético.', source: 'custom', path: '/sentinela-privada/notas/SKILL.md', receipt: null, installationId: `${NS}:1`, contentStatus: 'verified' })
const curada = (): InstalledSkillV2 => ({ ...manual(), source: 'catalog', receipt: {
  schema: 1, catalogId: 'pacote', digest: 'a'.repeat(64), result: 'imported', coverage: 'pinnedPassiveTextOnly',
  origin: { repo: 'sintetico/pacote', revision: 'a'.repeat(40), path: 'skills/notas', sourceDigest: 'b'.repeat(64) },
  adaptation: null, license: 'MIT', limitations: [], files: [arquivo(), arquivo('references/exemplo.md', 30)],
} })

// Componentes reais em SSR; esbuild já é usado pelos testes de views do app. Sem DOM/rede.
const bundle = await build({
  stdin: { contents: `export { ResumoInformativo } from './ResumoInformativo'; export { ConferenciaSkill, UsoDaSkill } from './Acabamento'; export { DetalhesSkill } from './DetalhesSkill';
    export { StoreContext } from '../../../store/context'; export { idiomaPref } from '../../../lib/fork-i18n/estado';
    export { createElement } from 'react'; export { renderToStaticMarkup } from 'react-dom/server';`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent',
  // Sem DOM não há `getServerSnapshot`: como em bot-ferramentas/tela.test.ts, o React do app usa o snapshot do cliente.
  plugins: [{ name: 'react-ssr', setup(b) {
    const real = createRequire(import.meta.url).resolve('react')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) }` }))
  } }],
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.svg': 'empty', '.png': 'empty', '.jpg': 'empty', '.webp': 'empty', '.mp3': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const module = { exports: {} }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), console,
  window: { addEventListener() {}, removeEventListener() {}, matchMedia: media, codync: { platform: 'linux' }, devicePixelRatio: 1 },
  navigator: { platform: 'Linux' }, matchMedia: media, TextEncoder, URL, URLSearchParams, Intl, setTimeout, clearTimeout, queueMicrotask,
  localStorage: { getItem: () => null, setItem() {}, removeItem() {} }, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
type M = typeof import('./ResumoInformativo') & typeof import('./Acabamento') & typeof import('./DetalhesSkill') & typeof import('../../../store/context')
  & { idiomaPref: { set(v: string): void } } & { createElement: typeof import('react').createElement; renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup }
const m = module.exports as M
m.idiomaPref.set('pt-BR')
const h = m.createElement
const html = (s: InstalledSkillV2, vigente = true, usoMedido = false) => m.renderToStaticMarkup(h(m.ResumoInformativo, { resumo: resumoDaInstalada(s, vigente), idioma: 'pt-BR', usoMedido }))
const conferencia = (estado: EstadoConferencia, s = curada()) => m.renderToStaticMarkup(h(m.ConferenciaSkill, { estado, recibo: s.receipt, livre: true, onVerificar: async () => true }))

/** Hub sintético com os RPCs de leitura existentes; qualquer outro método é falha. */
function hubSintetico(instaladas: () => InstalledSkillV2[]) {
  const chamadas: string[] = []
  const client: SkillClient = { async call<T>(metodo: string): Promise<T> {
    chamadas.push(metodo)
    if (metodo === 'marketSkills') return { skillProtocol: 2, items: [] } as T
    if (metodo === 'skills') return { skillProtocol: 2, items: structuredClone(instaladas()) } as T
    throw new Error(`método inesperado: ${metodo}`)
  } }
  return { client, chamadas }
}
/** O mesmo objeto que a folha recebe por `useStore()` e a loja lê por `catalogoFor` (a forma do BotStore). */
type Conexao = { hello: { hostId: string } | null; connectedClient: SkillClient | null; principalEpoch: number; bots: Map<string, unknown>; subscribe: (l: () => void) => () => void; getVersion: () => number }
const conexao = (client: SkillClient): Conexao => ({ hello: { hostId: 'hub-a' }, connectedClient: client, principalEpoch: 1, bots: new Map(), subscribe: () => () => {}, getVersion: () => 0 })

test('resumo: conserva o manifesto do recibo instalado; não atesta auxiliares ou versão mais nova', () => {
  const s = curada(), resumo = resumoDaInstalada(s, true)
  assert.deepEqual(resumo.arquivos?.map((f) => f.path), ['SKILL.md', 'references/exemplo.md'])
  s.receipt!.files.push(arquivo('outro.md'))
  assert.equal(resumo.arquivos?.length, 2, 'projeção sem referência mutável à lista recebida')
  assert.equal(resumo.endereco, 'Skills / notas / SKILL.md')
  assert.deepEqual(Object.keys(resumo).sort(), ['arquivos', 'endereco', 'origem', 'vigente'])
  assert.match(conferencia('verified'), /Igual ao recibo/)
  assert.match(conferencia('verified'), /SKILL\.md.*Não atesta os arquivos auxiliares/)
  assert.doesNotMatch(html(s), /Igual ao recibo|Confere|Texto principal/, 'o resumo não tem selo próprio')
})

test('resumo: estados neutros, um só selo/botão; uso real só vigente; sem data, zero ou nunca inventados', () => {
  assert.equal(resumoDaInstalada(manual(), true).origem, 'manual')
  assert.equal(resumoDaInstalada({ ...manual(), source: 'legado' }, true).origem, 'legado')
  for (const [estado, rotulo] of [['verified', 'Igual ao recibo'], ['changed', 'Diferente do registrado'], ['unavailable', 'Não conferido'], ['semRecibo', 'Sem recibo para comparar'], ['anterior', 'Leitura anterior · não confirmada nesta conexão']] as const) {
    const texto = conferencia(estado)
    assert.ok(texto.includes(rotulo), rotulo)
    assert.equal((texto.match(/Verificar agora/g) ?? []).length, 1)
    assert.doesNotMatch(texto, /depois da instalação|não conseguiu ler|Agora, às/)
    if (estado !== 'verified') assert.doesNotMatch(texto, /Igual ao recibo/)
  }
  // Medição: ausente e vigente = Sem medição; leitura anterior = Desconhecido; com medição, a célula sai e o Uso existente mostra.
  assert.equal((html(manual()).match(/Sem medição/g) ?? []).length, 3)
  assert.match(html(manual()), /Uso confirmado/)
  const anterior = html(manual(), false)
  assert.equal((anterior.match(/Sem medição/g) ?? []).length, 2)
  assert.match(anterior, /Uso confirmado<\/span><span[^>]*>Desconhecido</)
  assert.doesNotMatch(anterior, /Skills \/ notas|Texto manual/)
  assert.doesNotMatch(html(manual(), true, true), /Uso confirmado/)
  assert.equal(m.renderToStaticMarkup(h(m.UsoDaSkill, { uso: null })), '', 'sem campo do Hub, nem zero nem "nunca"')
  for (const texto of [html(manual()), anterior]) assert.doesNotMatch(texto, /Nunca usada|0 vezes|fora do app|revisão 1/)
})

test('resumo: folha inteira sem dados privados; manifesto hostil/incompleto não vira lista parcial', async () => {
  const s = { ...curada(), name: 'Notas sintéticas', description: 'Descrição sintética.', usage: { lastUsedAt: 1_700_000_000_000, count: 3 } }
  for (const sentinela of [s.path, s.installationId, NS]) assert.ok(!html(s).includes(sentinela), sentinela)
  // Folha real (DetalhesSkill) montada com a loja real e o mesmo objeto de conexão.
  const hub = hubSintetico(() => [s])
  const vivo = conexao(hub.client)
  const loja = catalogoFor(vivo, async () => hub.client)
  loja.sincronizar('hub-a', { adiarCarga: true })
  await loja.carregar()
  const folha = () => m.renderToStaticMarkup(h(m.StoreContext.Provider, { value: vivo as never }, h(m.DetalhesSkill, { loja: loja as never, item: null, instalada: s })))
  const vigente = folha()
  for (const sentinela of [s.path, '/sentinela-privada', NS, ':1"']) assert.ok(!vigente.includes(sentinela), sentinela)
  assert.doesNotMatch(vigente, /data-installation|file:\/\/|<script|onclick=/)
  assert.equal((vigente.match(/Verificar agora/g) ?? []).length, 1, 'um só botão de conferência')
  assert.equal((vigente.match(/>Conferência</g) ?? []).length, 1)
  assert.doesNotMatch(vigente, /Igual ao recibo/, 'sem resposta de detalhes, não há positivo')
  assert.match(vigente, /Último uso/, 'medição real da mesma instalação aparece uma vez')
  assert.doesNotMatch(vigente, /Uso confirmado/)
  vivo.connectedClient = null // queda: connectedClient fica null na tolerância visual
  const queda = folha()
  assert.match(queda, /Leitura anterior · não confirmada nesta conexão/)
  assert.doesNotMatch(queda, /Último uso|3 vezes|Skills \/ notas/)
  assert.match(queda, /Uso confirmado<\/span><span[^>]*>Desconhecido</)
  assert.deepEqual(hub.chamadas.filter((c) => c !== 'marketSkills' && c !== 'skills'), [], 'sem RPC além das leituras existentes')

  assert.equal(resumoDaInstalada({ ...s, id: 'notas\n' }, true).endereco, null)
  const invalidos: unknown[] = [[], null, [arquivo('x.md')], [arquivo(), arquivo('references/Notas.md'), arquivo('references/notas.md')],
    ...['../x.md', '/x.md', 'C:/x.md', '\\\\host/x.md', 'x/../y.md', 'x\u202emd', 'x\0.md', 'x\n', 'a//b.md', 'x/'.repeat(6) + 'y.md', 'a'.repeat(101)].map((p) => [arquivo(), arquivo(p)]),
    [arquivo(), { ...arquivo('x.md'), sha256: 'a'.repeat(64) + '\n' }], [arquivo(), { ...arquivo('x.md'), mode: '100755' }],
    [arquivo(), arquivo('x.md', -1)], [arquivo(), arquivo('x.md', 1024 * 1024 + 1)], [arquivo(), arquivo('x.md', 0.5)],
    [arquivo(), ...Array.from({ length: 5 }, (_, i) => arquivo(`x${i}.md`, 1024 * 1024))],
    Array.from({ length: 101 }, (_, i) => arquivo(i ? `x${i}.md` : 'SKILL.md', 0)),
  ]
  for (const raw of invalidos) assert.equal(manifestoApresentavel(raw), null)
  assert.equal(manifestoApresentavel(Array.from({ length: 100 }, (_, i) => arquivo(i ? `x${i}.md` : 'SKILL.md', 0)))?.length, 100)
  assert.deepEqual(manifestoApresentavel([{ ...arquivo(), privatePath: s.path, installationId: s.installationId }]), [arquivo()])
})

test('resumo: predicado real da folha; cada guarda retira o positivo (queda, hello, época, A→B→A, lista, reinstalação, resposta tardia)', async () => {
  let lista = [curada()]
  const hub = hubSintetico(() => lista)
  const vivo = conexao(hub.client)
  const loja: CatalogoStore = catalogoFor(vivo, async () => hub.client)
  loja.sincronizar('hub-a', { adiarCarga: true })
  await loja.carregar()
  const aberta = lista[0]!, origem = loja.capturarIntencao()
  const versaoAberta = versaoDaFolha(null, aberta)
  /** Os sinais exatamente como `DetalhesSkill` os deriva, a partir da lista atual e de uma resposta de detalhes. */
  const sinais = (resposta: SkillDetailsV2 | null, recusa: SinaisDaFolha['conteudoRecusado'] = null): SinaisDaFolha => {
    const atual = loja.instaladas?.find((s) => s.id === aberta.id) ?? null
    const daVersao = resposta ? detalhesDaVersao(resposta, null, atual) : false
    return { mudou: versaoDaFolha(null, atual) !== versaoAberta, outraNoHub: resposta !== null && !daVersao, detalhesDaVersao: daVersao, conteudoRecusado: recusa }
  }
  const detalhes = (s: InstalledSkillV2): SkillDetailsV2 => ({ skillProtocol: 2, installation: s, markdown: 'sintético', catalog: null, receipt: s.receipt })
  const estado = (resposta: SkillDetailsV2 | null = detalhes(aberta), recusa: SinaisDaFolha['conteudoRecusado'] = null) => {
    const f = sinais(resposta, recusa)
    return estadoDaConferencia(leituraVigente(loja, vivo, origem, f), aberta.receipt, f)
  }
  const positivo = () => assert.equal(estado(), 'verified', 'controle positivo exato')

  positivo()
  assert.equal(estado(null), 'unavailable', 'sem resposta atual não há positivo')
  assert.equal(estado(detalhes(aberta), 'changed'), 'changed')
  assert.equal(estadoDaConferencia(true, null, sinais(detalhes(aberta))), 'semRecibo')
  // Resposta tardia de outra encarnação: outraNoHub derruba.
  assert.equal(estado(detalhes({ ...aberta, installationId: `${NS}:9` })), 'anterior')
  // Queda: connectedClient null (loja também perde a conexão).
  vivo.connectedClient = null
  assert.equal(estado(), 'anterior')
  vivo.connectedClient = hub.client; positivo()
  // Hello novo/época: principalEpoch muda.
  vivo.principalEpoch = 2
  assert.equal(estado(), 'anterior')
  vivo.principalEpoch = 1; positivo()
  // Só a conexão (BotStore) diverge da origem, com a loja ainda igual: o termo próprio também derruba.
  assert.equal(leituraVigente({ conexaoAtual: () => true, listasDaOrigem: () => true }, { ...vivo, hello: { hostId: 'hub-b' } }, origem, sinais(detalhes(aberta))), false)
  assert.equal(leituraVigente({ conexaoAtual: () => true, listasDaOrigem: () => true }, { ...vivo, principalEpoch: 7 }, origem, sinais(detalhes(aberta))), false)
  assert.equal(leituraVigente({ conexaoAtual: () => true, listasDaOrigem: () => true }, { ...vivo, connectedClient: hubSintetico(() => []).client }, origem, sinais(detalhes(aberta))), false)
  assert.equal(leituraVigente({ conexaoAtual: () => true, listasDaOrigem: () => false }, vivo, origem, sinais(detalhes(aberta))), false, 'lista de outra origem')
  // Abertura sem cliente: origem capturada durante a queda nunca vira positivo.
  vivo.connectedClient = null
  const semCliente = loja.capturarIntencao()
  vivo.connectedClient = hub.client
  assert.equal(leituraVigente(loja, vivo, semCliente, sinais(detalhes(aberta))), false)
  assert.equal(leituraVigente(loja, vivo, null, sinais(detalhes(aberta))), false)
  // Reinstalação na mesma ID: a lista relida traz outra encarnação; a folha retida fica "anterior".
  lista = [{ ...curada(), installationId: `${NS}:2` }]
  await loja.carregar()
  assert.equal(estado(detalhes(lista[0]!)), 'anterior')
  assert.equal(estado(), 'anterior')
  lista = [aberta]
  await loja.carregar()
  positivo()
  // A→B→A real: a loja troca de Hub e volta; a origem da folha retida não é readotada.
  vivo.hello = { hostId: 'hub-b' }; loja.sincronizar('hub-b', { adiarCarga: true })
  vivo.hello = { hostId: 'hub-a' }; loja.sincronizar('hub-a', { adiarCarga: true })
  await loja.carregar()
  assert.equal(estado(), 'anterior')
  // Reconexão no mesmo Hub com nova geração e lista relida: a origem antiga continua "anterior".
  vivo.principalEpoch = 3
  await loja.carregar()
  assert.equal(estado(), 'anterior')
  assert.deepEqual(hub.chamadas.filter((c) => c !== 'marketSkills' && c !== 'skills'), [], 'nenhum RPC além das leituras existentes')
})
