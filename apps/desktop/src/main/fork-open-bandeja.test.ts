import assert from 'node:assert/strict'
import test from 'node:test'
import { openTrayMenu, openTraySummary } from './fork-open/bandeja-menu.ts'

const bot = { ref: 'a/b', name: 'Berga', detail: 'Pronto', working: false }
const calls: string[] = []
const actions = {
  show: () => calls.push('show'),
  send: (command: { kind: string }, front: boolean) => calls.push(`${command.kind}:${front}`),
  quit: () => calls.push('quit'),
}
const rotulos = (items: { label?: string }[]) => items.map((i) => i.label).filter(Boolean)

test('o menu da Open só tem o que o perfil oferece', () => {
  const items = openTrayMenu({ status: 'Conectado', bots: [bot] }, 'Mexerica Open', 'pt-BR', actions)
  assert.deepEqual(rotulos(items), ['Conectado', 'Abrir a Mexerica Open', 'Berga', 'Ajustes', 'Sair da Mexerica Open'])
  assert.ok(!JSON.stringify(rotulos(items)).match(/parear|tela|atualiza|cofre/i))
})

test('sem resumo o estado é de conexão e não há bots', () => {
  const items = openTrayMenu(null, 'Mexerica Open', 'pt-BR', actions)
  assert.deepEqual(rotulos(items).slice(0, 3), ['Conectando…', 'Abrir a Mexerica Open', 'Nenhum bot ainda'])
})

test('os rótulos da bandeja seguem o idioma efetivo', () => {
  const emIngles = openTrayMenu(null, 'Mexerica', 'en', actions)
  assert.deepEqual(rotulos(emIngles).slice(0, 2), ['Connecting…', 'Open Mexerica'])
})

test('parar tarefa só aparece com o bot trabalhando e as ações chegam ao destino', () => {
  const sub = (b: typeof bot) => (openTrayMenu({ status: 's', bots: [b] }, 'M', 'pt-BR', actions).find((i) => i.label === b.name)!.submenu as { label?: string; click?: () => void }[])
  assert.equal(sub(bot).length, 2)
  const trabalhando = sub({ ...bot, working: true })
  assert.equal(trabalhando.length, 3)
  calls.length = 0
  trabalhando[1].click!(); trabalhando[2].click!()
  const items = openTrayMenu(null, 'M', 'pt-BR', actions) as { label?: string; click?: () => void }[]
  items[1].click!(); items.find((i) => i.label === 'Ajustes')!.click!(); items.at(-1)!.click!()
  assert.deepEqual(calls, ['openBot:true', 'stopBot:false', 'show', 'openSettings:true', 'quit'])
})

test('resumo fora da forma esperada é descartado', () => {
  assert.deepEqual(openTraySummary({ status: 's', bots: [bot, { ...bot, avatar: 'x', extra: 1 }] })?.bots[1], bot)
  for (const ruim of [null, 'x', {}, { status: 1, bots: [] }, { status: 's', bots: [{ ...bot, working: 'sim' }] }, { status: 's', bots: [{ ...bot, ref: 'x'.repeat(201) }] }]) {
    assert.equal(openTraySummary(ruim), null)
  }
})

test('textos longos são encurtados e a lista passa de 50 sem descartar o resumo', () => {
  const longo = 'x'.repeat(500)
  const resumo = openTraySummary({ status: longo, bots: Array.from({ length: 60 }, (_, i) => ({ ...bot, ref: `a/${i}`, name: longo, detail: longo })) })!
  assert.equal(resumo.bots.length, 50)
  assert.equal(resumo.bots[49].ref, 'a/49')
  for (const texto of [resumo.status, resumo.bots[0].name, resumo.bots[0].detail]) assert.equal(texto.length, 200)
  assert.equal(openTraySummary({ status: 's', bots: [{ ...bot, ref: 'x'.repeat(201) }] }), null)
})
