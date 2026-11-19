import assert from 'node:assert/strict'
import test from 'node:test'
import type { Bot } from '../../shared/models'
import { groupRoster } from './bot-groups.ts'
import { matchesWords, searchWords } from './busca.ts'
import { Observable } from './observable.ts'
import { PrincipalModel } from '../store/principal.ts'
import { linhaDoPedido, pedidoAberto, principalEmpty, principalListEmpty, shownHubsOffline, principalStatus, principalTrigger, projectPrincipal, protectedPrincipal } from './principal.ts'
import { tradutor } from './fork-i18n/index.ts'

const t = tradutor('pt-BR')

test('estados do atendimento preservam prioridade, pedido humano, cota e erro', () => {
  for (const [status, text] of [['idle', 'Livre'], ['working', 'Ocupado'], ['needsInput', 'Precisa de você'], ['waiting', 'Aguardando cota'], ['error', 'Não foi possível concluir']]) {
    assert.equal(principalStatus(t, 'pt-BR', { status: status! }, true).text, text)
  }
  assert.deepEqual(principalStatus(t, 'pt-BR', { status: 'waiting' }, true, true), { text: 'Precisa de você', secondary: 'Aguardando cota', tone: 'attention' })
  assert.deepEqual(principalStatus(t, 'pt-BR', { status: 'error' }, true, true), { text: 'Precisa de você', secondary: 'Não foi possível concluir', tone: 'attention' })
  const offline = principalStatus(t, 'pt-BR', { status: 'waiting' }, false, true)
  assert.equal(offline.text, 'Sem conexão com o Hub')
  assert.equal(offline.secondary, 'Último estado: Precisa de você · Aguardando cota')
  assert.equal(principalEmpty(t, { id: null, phase: 'ready' }, false), 'Sem conexão com o Hub')
  assert.equal(principalEmpty(t, { id: null, phase: 'loading' }, true), 'Carregando o principal…')
  assert.equal(principalEmpty(t, { id: null, phase: 'error' }, true), 'Não foi possível consultar o principal')
  assert.equal(principalEmpty(t, { id: null, phase: 'ready' }, true), 'Não foi possível consultar o principal')
  assert.equal(principalEmpty(t, { id: null, phase: 'unknown' }, false, true), 'Atualize o app para ver o principal')
  assert.equal(principalStatus(t, 'pt-BR', { status: 'idle' }, false, false, true).text, 'Atualize o app para ver o principal')
  // runtime-audiencia-01: sem runtime aqui não é Livre nem Ocupada; sem conexão continua vencendo.
  for (const status of ['unavailable', 'paused']) assert.deepEqual(principalStatus(t, 'pt-BR', { status }, true), { text: 'Estado indisponível', tone: 'unknown' })
  assert.equal(principalStatus(t, 'pt-BR', { status: 'unavailable' }, false).secondary, 'Último estado: Estado indisponível')
  assert.equal(principalStatus(t, 'pt-BR', { status: 'unavailable' }, true, true).text, 'Precisa de você')
  assert.equal(principalListEmpty(t, false, false), 'Aguardando um computador')
  assert.equal(principalListEmpty(t, true, false), 'Aguardando um computador')
})

test('cota mostra a retomada no fuso local, sem perder a prioridade do pedido humano', () => {
  const bot = { status: 'waiting', resumeAt: new Date(2026, 9, 22, 15, 30).getTime() }
  assert.deepEqual(principalStatus(t, 'pt-BR', bot, true), { text: 'Aguardando cota', secondary: 'retomada às 15:30', tone: 'normal' })
  assert.deepEqual(principalStatus(t, 'pt-BR', bot, true, true), { text: 'Precisa de você', secondary: 'Aguardando cota · retomada às 15:30', tone: 'attention' })
  assert.equal(principalStatus(t, 'pt-BR', bot, false, true).secondary, 'Último estado: Precisa de você · Aguardando cota · retomada às 15:30')
})

test('identidade provisória restringe a lista na fase unknown, só para o mesmo Hub', () => {
  const saved = { hostId: 'host-um', primaryBotId: 'a' }
  const unknown = { id: null, phase: 'unknown' as const }
  assert.equal(protectedPrincipal(unknown, saved, 'host-um'), 'a')
  assert.deepEqual(projectPrincipal([{ bot: { id: 'a' } }, { bot: { id: 'b' } }], protectedPrincipal(unknown, saved, 'host-um')).others, [{ bot: { id: 'b' } }])
  assert.equal(protectedPrincipal(unknown, saved, 'host-dois'), null)
  assert.equal(protectedPrincipal(unknown, saved, null), null)
  assert.equal(protectedPrincipal({ id: null, phase: 'ready' }, saved, 'host-um'), null)
  assert.equal(protectedPrincipal({ id: 'nova', phase: 'ready' }, saved, 'host-um'), 'nova')
})

test('principal oculta entra na paleta por ID; homônimo, busca e seção não a duplicam', () => {
  const principal = { bot: { id: 'principal', name: 'Donna', hidden: true } }
  const other = { bot: { id: 'homonima', name: 'Donna', hidden: false } }
  const result = projectPrincipal([other], principal.bot.id, principal)
  assert.deepEqual(result.palette, [other, principal])
  assert.deepEqual(result.others, [other])
  assert.deepEqual(result.palette.filter((item) => matchesWords(searchWords('donna'), [item.bot.name])), [other, principal])
  assert.deepEqual(result.others.filter((item) => matchesWords(searchWords('inexistente'), [item.bot.name])), [])
  assert.equal(principalListEmpty(t, true), 'Nenhum outro bot encontrado')
  assert.equal(principalListEmpty(t, false), 'Seus outros bots aparecem aqui')
  assert.equal(projectPrincipal([principal, other], principal.bot.id, principal).palette.length, 2)
})

test('trocar principal conserva as atribuições e seções vazias; a anterior volta à seção', () => {
  const items = ['a', 'b'].map((id) => ({ bot: { id } }))
  const snapshot = { rev: 1, groups: [{ id: 'trabalho', name: 'Trabalho', bots: ['a'] }] }
  const before = structuredClone(snapshot)
  const groups = groupRoster(projectPrincipal(items, 'a').others, snapshot)
  assert.deepEqual(groups.map((group) => [group.id, group.items.map((item) => item.bot.id)]), [['fora-de-grupo', ['b']], ['trabalho', []]])
  const after = groupRoster(projectPrincipal(items, 'b').others, snapshot)
  assert.deepEqual(after[1]!.items, [items[0]])
  assert.deepEqual(snapshot, before)
})

test('projeção por Hub separa botIds iguais', () => {
  const hubs = ['um', 'dois'].map((computerId) => ({ computerId, items: [{ bot: { id: 'igual' }, ref: { computerId, botId: 'igual' } }] }))
  const projected = hubs.map((hub) => ({ ...hub, ...projectPrincipal(hub.items, 'igual', hub.items[0]) }))
  assert.deepEqual(projected.map((hub) => hub.others.length), [0, 0])
  assert.deepEqual(projected.flatMap((hub) => hub.palette.map((item) => item.ref)), [{ computerId: 'um', botId: 'igual' }, { computerId: 'dois', botId: 'igual' }])
})

const tick = () => new Promise<void>((resolve) => setImmediate(resolve))
const bot = (id: string, manageTeam = false) => ({ id, manageTeam, status: 'idle', rev: 1 }) as Bot
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((yes) => { resolve = yes })
  return { promise, resolve }
}
class Source extends Observable {
  computer = { id: 'computador-um' }
  contextId = 'local'
  hostId = 'host-um'
  bots = new Map([['a', bot('a')]])
  connection = { kind: 'online' }
  principalEpoch = 1
  mismatch = null
  calls = 0
  read = async () => ({ primaryBotId: 'a' as string | null })
  async teamSettings() { this.calls++; return this.read() }
  notify() { this.changed() }
}

function storage() {
  const disk = new Map<string, string>()
  return { getItem: (key: string) => disk.get(key) ?? null, setItem: (key: string, value: string) => { disk.set(key, value) } }
}
function modelFor(source: Source, disk = storage()) {
  return new PrincipalModel(source, { read: () => source.teamSettings(), hostId: () => source.hostId, storage: disk })
}

test('preferência sobrevive à reabertura, separa computadores e é descartada na troca do host', async () => {
  const disk = storage()
  const source = new Source()
  const confirmed = modelFor(source, disk)
  source.notify(); await tick()
  assert.equal(confirmed.id, 'a')
  confirmed.retire()

  source.connection = { kind: 'offline' }
  const reopened = modelFor(source, disk)
  assert.equal(reopened.id, null, 'preferência não confirma identidade nem habilita seletor')
  assert.equal(reopened.phase, 'unknown')
  assert.equal(reopened.protectedId, 'a')
  const other = new Source()
  other.computer.id = 'computador-dois'
  const otherModel = modelFor(other, disk)
  assert.equal(otherModel.protectedId, null)
  otherModel.retire()

  source.hostId = 'host-dois'; source.notify(); await tick()
  assert.equal(reopened.protectedId, null)
  source.hostId = 'host-um'; source.notify(); await tick()
  assert.equal(reopened.protectedId, null, 'a preferência antiga foi descartada, não apenas ignorada')
  reopened.retire()
  const discarded = modelFor(source, disk)
  assert.equal(discarded.protectedId, null)
  discarded.retire()
})

test('consulta só em conexão, criação/exclusão, manageTeam e gravação; nunca em token/status', async () => {
  const source = new Source()
  const model = modelFor(source)
  source.notify(); await tick()
  assert.equal(model.id, 'a')
  assert.equal(source.calls, 1)
  const signature = principalTrigger(source.bots.values())
  source.bots.set('a', { ...source.bots.get('a')!, rev: 100, status: 'working', hidden: true, position: 8 })
  source.notify(); await tick()
  assert.equal(principalTrigger(source.bots.values()), signature)
  assert.equal(source.calls, 1)
  source.bots.set('b', bot('b')); source.notify(); await tick()
  source.bots.set('b', bot('b', true)); source.notify(); await tick()
  source.bots.delete('b'); source.notify(); await tick()
  assert.equal(source.calls, 4)
  source.principalEpoch++; source.notify(); await tick()
  assert.equal(source.calls, 5, 'reconexão curta também consulta')
  model.saved({ primaryBotId: null })
  assert.equal(model.id, null)
  assert.equal(model.phase, 'ready')
  model.retire()
})

test('uma consulta em voo; desconectar/reconectar descarta geração antiga, sem vazio falso', async () => {
  const source = new Source()
  const first = deferred<{ primaryBotId: string | null }>()
  source.read = () => first.promise
  const model = modelFor(source)
  source.notify(); await tick()
  assert.equal(model.phase, 'loading')
  source.connection = { kind: 'offline' }; source.notify(); await tick()
  assert.equal(principalEmpty(t, model, false), 'Sem conexão com o Hub')
  source.connection = { kind: 'online' }; source.principalEpoch++; source.notify(); await tick()
  assert.equal(source.calls, 1)
  source.read = async () => ({ primaryBotId: 'nova' })
  first.resolve({ primaryBotId: 'antiga' }); await tick()
  assert.equal(source.calls, 2)
  assert.equal(model.id, 'nova')
  assert.equal(model.phase, 'ready')
  model.retire()
})

test('gravação e retirada do store invalidam resposta antiga; erro termina carregamento', async () => {
  const source = new Source()
  const first = deferred<{ primaryBotId: string | null }>()
  source.read = () => first.promise
  const model = modelFor(source)
  source.notify(); await tick()
  model.saved({ primaryBotId: 'escolhida' })
  first.resolve({ primaryBotId: 'velha' }); await tick()
  assert.equal(model.id, 'escolhida')
  source.read = async () => { throw new Error('falhou') }
  model.refresh(); await tick()
  assert.equal(model.phase, 'error')
  model.retire()
  source.principalEpoch++; source.notify(); await tick()
  assert.equal(source.calls, 2)
})

test('D249 R5: sem conexão, a lista vazia não diz nada (o lado da conversa diz uma vez); a busca ainda responde', () => {
  assert.equal(principalListEmpty(t, false, true, true), null)
  assert.equal(principalListEmpty(t, true, true, true), 'Nenhum outro bot encontrado')
  assert.equal(principalListEmpty(t, false, true, false), 'Seus outros bots aparecem aqui')
})

test('D249 R5: sem conexão é sobre os computadores mostrados; um oculto conectando ou online não segura o estado', () => {
  assert.equal(shownHubsOffline([{ kind: 'offline' }]), true)
  assert.equal(shownHubsOffline([{ kind: 'offline' }, { kind: 'computerOffline' }]), true)
  assert.equal(shownHubsOffline([{ kind: 'offline' }, { kind: 'connecting' }]), false, 'um mostrado ainda conectando')
  assert.equal(shownHubsOffline([{ kind: 'offline' }, { kind: 'online' }]), false)
  assert.equal(shownHubsOffline([{ kind: 'unauthorized' }]), false, 'sem autorização tem o próprio aviso')
  assert.equal(shownHubsOffline([]), false, 'nenhum computador na barra')
})

test('uma autorização só: o pedido sai do estado, nunca do texto da atividade', () => {
  // Cartão pendente nesta conversa: aprovação, mesmo com outro status.
  assert.equal(pedidoAberto({ status: 'working' }, true, false), 'aprovacao')
  // needsInput: aprovação, ou pergunta quando só há perguntas.
  assert.equal(pedidoAberto({ status: 'needsInput' }, false, false), 'aprovacao')
  assert.equal(pedidoAberto({ status: 'needsInput' }, false, true), 'pergunta')
  // Sem pedido no estado, nada (o texto que o Hub manda não conta).
  assert.equal(pedidoAberto({ status: 'working' }, false, false), null)
  assert.equal(pedidoAberto({ status: 'idle' }, false, false), null)
})

test('uma autorização só: o card do principal diz o pedido numa linha só', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    const t = tradutor(idioma)
    const precisa = principalStatus(t, idioma, { status: 'needsInput' }, true, true)
    assert.equal(linhaDoPedido(precisa, true, false, 'aprovacao'), 'aprovacao')
    assert.equal(linhaDoPedido(precisa, true, false, 'pergunta'), 'pergunta')
    // Sem Hub, com Hub desatualizado ou com cota na mesma linha, fica o texto de sempre.
    assert.equal(linhaDoPedido(principalStatus(t, idioma, { status: 'needsInput' }, false, true), false, false, 'aprovacao'), null)
    assert.equal(linhaDoPedido(precisa, true, true, 'aprovacao'), null)
    assert.equal(linhaDoPedido(principalStatus(t, idioma, { status: 'waiting', resumeAt: Date.now() + 60_000 }, true, true), true, false, 'aprovacao'), null)
    // Livre ou trabalhando: não pede nada.
    assert.equal(linhaDoPedido(principalStatus(t, idioma, { status: 'working' }, true), true, false, null), null)
  }
})
