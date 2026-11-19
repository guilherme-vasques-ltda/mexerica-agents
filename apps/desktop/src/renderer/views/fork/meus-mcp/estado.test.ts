import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { InstalledConnector } from '../../marketplace/market-models.ts'
import { detalheDe, filtrarLocal, LeituraConectores, resultadoIncerto, selosDe, type FonteConectores } from './estado.ts'

// D70 (D175): só as fronteiras de Meus MCP na UI. Conectores sintéticos; nenhum Hub, conta ou MCP real.
const c = (over: Partial<InstalledConnector>): InstalledConnector => ({ id: 'x', name: 'X', description: '', kind: 'local', keys: [], ...over })
const legado = c({ id: 'io.github.fixture-weather', name: 'Weather fictício', registryName: 'io.github.fixture/weather', command: 'npx -y @fixture/weather@1.2.3', configuration: 'configured' })
const incompleto = c({ id: 'catalogo-incompleto', name: 'Incompleto fictício', registryName: 'io.github.fixture/sem-alvo', configuration: 'manualRequired' })
const remoto = c({ id: 'remoto', name: 'Notas fictícias', kind: 'remote', url: 'https://mcp.fixture.invalid/mcp', auth: 'signedOut' })

test('Meus MCP retém legado do catálogo e incompleto', () => {
  const todos = [legado, incompleto, remoto]
  // Busca local só na lista do usuário (nome, descrição, comando e URL).
  assert.deepEqual(filtrarLocal(todos, 'NOTAS').map((x) => x.id), ['remoto'])
  assert.deepEqual(filtrarLocal(todos, '@fixture/weather').map((x) => x.id), ['io.github.fixture-weather'])
  assert.equal(filtrarLocal(todos, '  ').length, 3)
})

test('selos honestos: incompleto pede configuração manual, login pendente pede entrar, nada diz pronto', () => {
  assert.deepEqual(selosDe(legado), [])
  assert.deepEqual(selosDe(incompleto).map((s) => s.id), ['manual'])
  assert.deepEqual(selosDe(remoto).map((s) => s.id), ['entrar'])
  // Hub antigo sem `configuration`: só o alvo público decide.
  assert.deepEqual(selosDe(c({ url: 'https://x.invalid/{tenant}/mcp', kind: 'remote' })).map((s) => s.id), ['manual'])
  assert.deepEqual(detalheDe(incompleto), { chave: 'extensoes.mcp.detalhe.manual' })
  assert.deepEqual(detalheDe(remoto), { codigo: 'https://mcp.fixture.invalid/mcp' })
})

test('escrita sem resposta clara é incerta; recusa do Hub não é', () => {
  assert.equal(resultadoIncerto({ status: 0, message: 'timeout' }), true)
  assert.equal(resultadoIncerto(new Error('IPC caiu')), true)
  assert.equal(resultadoIncerto({ status: 400, message: 'mcp_registry_unavailable: …' }), false)
  assert.equal(resultadoIncerto({ status: 500, message: 'falhou' }), false)
})

function fonte(hub: string | null, client: object | null, geracao = 1): { -readonly [K in keyof FonteConectores<object>]: FonteConectores<object>[K] } {
  return { hello: hub ? { hostId: hub } : null, connectedClient: client, principalEpoch: geracao }
}

test('erro de leitura é erro, nunca lista vazia; sem link não mostra nada', async () => {
  const link = {}
  const f = fonte('hub-a', link)
  const leitura = new LeituraConectores<object>()
  assert.equal(leitura.estado(f), 'lendo')
  await leitura.ler(f, async () => { throw new Error('cofre trancado') })
  assert.equal(leitura.estado(f), 'erro')
  assert.equal(leitura.erro(f), 'cofre trancado')
  assert.deepEqual(leitura.itens(f), [])
  await leitura.ler(f, async () => [legado])
  assert.equal(leitura.estado(f), 'lida')
  f.connectedClient = null
  assert.equal(leitura.estado(f), 'semConexao')
  assert.deepEqual(leitura.itens(f), [])
})

test('troca de Hub, reconexão e resposta atrasada não publicam dados da origem anterior', async () => {
  const linkA = {}
  const f = fonte('hub-a', linkA)
  const leitura = new LeituraConectores<object>()
  await leitura.ler(f, async () => [legado])
  // Reconexão no mesmo Hub (nova geração): a lista anterior some até reler.
  f.principalEpoch = 2
  assert.deepEqual(leitura.itens(f), [])
  assert.equal(leitura.estado(f), 'lendo')
  // Resposta atrasada: começou no Hub A e termina depois da troca para o Hub B.
  let soltar!: (v: InstalledConnector[]) => void
  const atrasada = leitura.ler(f, () => new Promise((r) => { soltar = r }))
  f.hello = { hostId: 'hub-b' }
  f.connectedClient = {}
  soltar([incompleto])
  assert.equal(await atrasada, false)
  assert.deepEqual(leitura.itens(f), [])
  // Um pedido superado por outro mais novo também não publica.
  let primeiro!: (v: InstalledConnector[]) => void
  const velho = leitura.ler(f, () => new Promise((r) => { primeiro = r }))
  await leitura.ler(f, async () => [remoto])
  primeiro([legado])
  assert.equal(await velho, false)
  assert.deepEqual(leitura.itens(f).map((x) => x.id), ['remoto'])
})
