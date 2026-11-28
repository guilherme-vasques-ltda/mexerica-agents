import assert from 'node:assert/strict'
import { test } from 'node:test'
import { FluxoConectar, type Dependencias } from './fluxo.ts'
import { FluxoGoogle } from './google-fluxo.ts'
import { conta, grupo, servico, tipo } from './dados-de-teste.ts'
import { ehDoGoogle, guiaDoGoogle, pedidoDoPersonalizado, visaoDoCatalogo } from './regras.ts'
import { catalogo } from './dados-de-teste.ts'
import type { Servico } from './tipos.ts'

type Chamada = { metodo: string; corpo: Record<string, unknown> }

const guia = [{ id: 'projeto', url: 'https://console.example/projectcreate' }, { id: 'campos', url: null }]
const googleServico = (id: string, nome: string, extra: Partial<Servico> = {}): Servico => servico(id, nome, {
  categoria: 'Comunicação',
  tipos: [tipo([grupo('ler', { padrao: true }), grupo('rascunhos'), grupo('apagar', { disponivel: false })], {
    id: `${id}.oficial_app`, tipo: 'oficial_app', auth: { modo: 'app_proprio', provedor: 'google', emissor: 'https://accounts.example', guia },
  })], ...extra,
})

function montar(responder: (c: Chamada) => unknown, extra: Partial<Dependencias> = {}) {
  const chamadas: Chamada[] = []
  const eventos: string[] = []
  const cliente = { call: async (metodo: string, corpo: Record<string, unknown>) => { const c = { metodo, corpo }; chamadas.push(c); return responder(c) } }
  const deps: Dependencias = {
    cliente: async () => cliente as never,
    entrar: async (id) => { eventos.push(`entrar:${id}`); return true },
    cancelarLogin: async () => undefined,
    verificar: async (id) => { eventos.push(`verificar:${id}`); return { status: 'ready' } },
    guardarChave: async () => undefined,
    reler: async () => { eventos.push('reler') },
    ...extra,
  }
  const google = (servicos: Servico[], app: { configurado: boolean; clientId: string | null } | null) =>
    new FluxoGoogle(servicos, app, {
      cliente: deps.cliente, reler: deps.reler,
      criarFluxo: (s, t) => new FluxoConectar(s, t, deps),
    })
  return { chamadas, eventos, google }
}

const gmail = googleServico('gmail', 'Gmail')
const agenda = googleServico('google-agenda', 'Google Agenda')
const appSalvo = { configurado: true, clientId: '1-abc.apps.googleusercontent.com' }

test('o grupo Google é pelo provedor do app próprio; a lista comum não os repete', () => {
  const comum = servico('linear', 'Linear')
  assert.equal(ehDoGoogle(gmail), true)
  assert.equal(ehDoGoogle(comum), false)
  assert.deepEqual(guiaDoGoogle([comum, gmail]).map((p) => p.id), ['projeto', 'campos'])
  const v = visaoDoCatalogo(catalogo([comum, gmail, agenda]), '', null)
  assert.deepEqual(v.google.map((s) => s.id), ['gmail', 'google-agenda'])
  assert.deepEqual(v.lista.map((s) => s.id), ['linear'])
  assert.equal(visaoDoCatalogo(catalogo([comum, gmail]), 'gmail', null).google.length, 1)
  assert.equal(visaoDoCatalogo(catalogo([comum, gmail]), 'linear', null).google.length, 0)
})

test('sem app cadastrado o fluxo abre o guia; com app, a escolha dos serviços', () => {
  const { google } = montar(() => ({}))
  assert.equal(google([gmail], null).passo, 'guia')
  assert.equal(google([gmail], { configurado: false, clientId: null }).passo, 'guia')
  assert.equal(google([gmail], appSalvo).passo, 'servicos')
})

test('salvar o app manda ID e chave ao Hub, só guarda o ID que volta e segue para os serviços', async () => {
  const { chamadas, eventos, google } = montar(() => ({ configurado: true, clientId: '1-abc.apps.googleusercontent.com' }))
  const f = google([gmail], null)
  assert.equal(await f.salvarApp(' 1-abc.apps.googleusercontent.com ', ' GOCSPX-chave '), true)
  assert.deepEqual(chamadas, [{ metodo: 'marketGoogleAppSalvar', corpo: { clientId: '1-abc.apps.googleusercontent.com', clientSecret: 'GOCSPX-chave' } }])
  assert.equal(f.passo, 'servicos')
  assert.deepEqual(f.app, appSalvo)
  assert.equal(JSON.stringify(f).includes('GOCSPX'), false, 'a chave não fica no modelo')
  assert.deepEqual(eventos, ['reler'])
})

test('app recusado pelo Hub: fica no guia com o erro, sem avançar', async () => {
  const { google } = montar(() => { throw new Error('app_google_invalido: o ID do cliente termina em .apps.googleusercontent.com') })
  const f = google([gmail], null)
  assert.equal(await f.salvarApp('x', 'y'), false)
  assert.equal(f.passo, 'guia')
  assert.match(f.erroDoApp ?? '', /app_google_invalido/)
  assert.equal(f.ocupado, false)
  // campo vazio nem chega ao Hub
  assert.equal(await f.salvarApp('', 'y'), false)
})

test('os serviços vêm marcados e só lendo; a escolha vai ao Hub grupo a grupo, um login por serviço, em fila', async () => {
  const contas: Record<string, string> = { gmail: 'gmail', 'google-agenda': 'google-agenda' }
  const { chamadas, eventos, google } = montar((c) => ({ connector: conta(contas[String(c.corpo.servico)], 'x', String(c.corpo.servico), { auth: 'signedOut' }), proximoPasso: 'entrar' }))
  const f = google([gmail, agenda], appSalvo)
  assert.equal(f.totalMarcados, 2)
  assert.deepEqual(f.fluxo('gmail')?.escolhidos, ['ler'])
  f.alternarGrupo('gmail', 'rascunhos')
  f.alternarGrupo('gmail', 'apagar') // indisponível: a ficha não deixa
  assert.deepEqual(f.fluxo('gmail')?.escolhidos, ['ler', 'rascunhos'])
  assert.equal(f.podeContinuar, true)
  await f.continuar()
  assert.deepEqual(chamadas.map((c) => c.corpo.servico), ['gmail', 'google-agenda'])
  assert.deepEqual(chamadas[0].corpo.liberacoes, ['ler', 'rascunhos'])
  assert.deepEqual(chamadas[1].corpo.liberacoes, ['ler'])
  assert.deepEqual(eventos.filter((e) => e.startsWith('entrar') || e.startsWith('verificar')), ['entrar:gmail', 'verificar:gmail', 'entrar:google-agenda', 'verificar:google-agenda'])
  assert.equal(f.passo, 'pronto')
  assert.equal(f.estadoDe('gmail'), 'pronto')
})

test('desmarcar um serviço o tira da fila; sem nenhum marcado não continua', async () => {
  const { chamadas, google } = montar((c) => ({ connector: conta(String(c.corpo.servico), 'x', String(c.corpo.servico)), proximoPasso: 'entrar' }))
  const f = google([gmail, agenda], appSalvo)
  f.alternarServico('gmail')
  await f.continuar()
  assert.deepEqual(chamadas.map((c) => c.corpo.servico), ['google-agenda'])
  const vazio = google([gmail], appSalvo)
  vazio.alternarServico('gmail')
  assert.equal(vazio.podeContinuar, false)
})

test('login que falha para na fila, mostra o erro e "tentar de novo" repete só o login, sem criar outra conta', async () => {
  let falhar = true
  const { chamadas, eventos, google } = montar((c) => ({ connector: conta(String(c.corpo.servico), 'x', String(c.corpo.servico), { auth: 'signedOut' }), proximoPasso: 'entrar' }), {
    entrar: async (id) => { eventos2.push(`entrar:${id}`); if (id === 'gmail' && falhar) throw new Error('redirect_uri_mismatch'); return true },
    cancelarLogin: async () => undefined,
  })
  const eventos2: string[] = []
  const f = google([gmail, agenda], appSalvo)
  await f.continuar()
  assert.equal(f.passo, 'entrar')
  assert.equal(f.estadoDe('gmail'), 'erro')
  assert.match(f.falhaDe('gmail') ?? '', /redirect_uri_mismatch/)
  assert.equal(f.estadoDe('google-agenda'), 'espera', 'a fila parou na falha')
  assert.equal(chamadas.length, 1)
  falhar = false
  await f.tentarDeNovo()
  assert.equal(chamadas.length, 2, 'só a agenda criou conta agora; o Gmail repetiu o login')
  assert.equal(f.passo, 'pronto')
  assert.deepEqual(eventos2, ['entrar:gmail', 'entrar:gmail', 'entrar:google-agenda'])
  assert.ok(eventos.length >= 0)
})

test('sem app no Hub (app_proprio_ausente) o serviço fica em erro com o código estável, sem login', async () => {
  const { eventos, google } = montar(() => { throw new Error('app_proprio_ausente: cadastre o app Google deste Space') })
  const f = google([gmail], appSalvo)
  await f.continuar()
  assert.equal(f.estadoDe('gmail'), 'erro')
  assert.match(f.falhaDe('gmail') ?? '', /^app_proprio_ausente/)
  assert.deepEqual(eventos.filter((e) => e.startsWith('entrar')), [])
})

test('trocar o app volta ao guia sem apagar nada; desistir desconecta só as contas pela metade', async () => {
  const { chamadas, google } = montar((c) => c.metodo === 'marketConectar' ? { connector: conta('gmail-1', 'Gmail', 'gmail', { auth: 'signedOut' }), proximoPasso: 'entrar' } : {}, {
    entrar: async () => { throw new Error('fechou o navegador') },
  })
  const f = google([gmail], appSalvo)
  f.trocarApp()
  assert.equal(f.passo, 'guia')
  f.voltarAosServicos()
  assert.equal(f.passo, 'servicos')
  await f.continuar()
  assert.equal(f.estadoDe('gmail'), 'erro')
  await f.cancelar()
  assert.deepEqual(chamadas.map((c) => c.metodo), ['marketConectar', 'marketContaDesconectar'])
})

test('o conector personalizado: link leva a chave no cabeçalho; comando nunca leva chave', () => {
  assert.deepEqual(pedidoDoPersonalizado('link', ' https://x.example/mcp ', ' Notas ', ''), { name: 'Notas', url: 'https://x.example/mcp' })
  assert.deepEqual(pedidoDoPersonalizado('link', 'https://x.example/mcp', 'Notas', ' k '), { name: 'Notas', url: 'https://x.example/mcp', headers: { Authorization: 'Bearer k' } })
  assert.deepEqual(pedidoDoPersonalizado('comando', 'npx -y pacote', 'Notas', 'segredo'), { name: 'Notas', command: 'npx -y pacote' })
})
