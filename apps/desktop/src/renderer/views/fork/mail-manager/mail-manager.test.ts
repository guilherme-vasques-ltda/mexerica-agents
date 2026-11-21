import assert from 'node:assert/strict'
import test from 'node:test'
import { EstadoMailManager, type Chamador } from './estado.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { textoDaMensagem, type Mensagem } from '../bot-settings/mensagem.ts'
import { CAIXAS_PREVIA, caixasPreviaFaltantes, conferirLeitura, contagemAutorizados, erroDoHub, parseAddressesInput, permissoesDoBot, type CaixaMail, type LeituraMailManager, type PermissaoMail } from './regras.ts'

// Mail Manager: as regras que a tela repete do Hub e o estado de leitura/erro. As mensagens são dado; o teste
// confere o texto pt-BR que a tela mostra.
const t = tradutor('pt-BR')
const msg = (m: Mensagem) => textoDaMensagem(t, m)
const espera = () => new Promise((r) => setTimeout(r, 0))
// O assert.equal do node:assert/strict estreita o tipo de estado.leitura para null; a função devolve o tipo cheio.
const atual = (e: EstadoMailManager) => e.leitura

const leitura = (sobre: Partial<LeituraMailManager> = {}): LeituraMailManager => ({
  plugin: { enabled: true, mode: 'preview' },
  transportAvailable: true,
  personGrantsSupported: false,
  bots: [
    { botId: 'donna', name: 'Berga', isPrimary: true },
    { botId: 'lia', name: 'Lia', isPrimary: false },
  ],
  mailboxes: [],
  ...sobre,
})

const caixa = (sobre: Partial<CaixaMail> = {}): CaixaMail => ({ id: 'c1', label: 'Suporte', addresses: ['suporte@loja.example.invalid'], grants: [], ...sobre })

test('conferirLeitura aceita a leitura do Hub e recusa qualquer formato inesperado', () => {
  assert.equal(conferirLeitura(leitura()).mailboxes.length, 0)
  // Campos extras do Hub (como connectedMailboxId na resposta de conectar) continuam válidos.
  assert.equal(conferirLeitura({ ...leitura(), connectedMailboxId: 'c9' }).bots.length, 2)
  assert.throws(() => conferirLeitura(null), /resposta_invalida/)
  assert.throws(() => conferirLeitura({}), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), plugin: { enabled: true, mode: 'full' } }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), plugin: { enabled: 'on', mode: 'preview' } }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), transportAvailable: 'yes' }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), personGrantsSupported: 1 }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), bots: [{ botId: 'x', name: 'X' }] }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), mailboxes: [{ id: 'c1', label: 'L', addresses: 'a@b.example.invalid', grants: [] }] }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), mailboxes: [{ id: 'c1', label: 'L', addresses: [], grants: [{ participantKind: 'alien', participantId: 'donna', view: false, send: false, delete: false }] }] }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({ ...leitura(), mailboxes: [{ id: 'c1', label: 'L', addresses: [], grants: [{ participantKind: 'bot', participantId: 7, view: false, send: false, delete: false }] }] }), /resposta_invalida/)
})

test('erroDoHub mapeia os códigos do Hub para chaves claras', () => {
  assert.match(msg(erroDoHub(new Error('mail_denied: o Hub recusou.'))), /recusou o pedido/)
  assert.match(msg(erroDoHub(new Error('mail_invalid: caixa sem endereço.'))), /não é válido/)
  assert.match(msg(erroDoHub(new Error('mail_storage_unavailable: o armazenamento caiu.'))), /não está disponível/)
  assert.match(msg(erroDoHub(new Error('not_allowed: só o dono.'))), /Só o dono/)
  assert.match(msg(erroDoHub({ status: 404, message: 'unknown method' })), /ainda não tem o Mail Manager/)
  assert.match(msg(erroDoHub(new Error('resposta_invalida'))), /não entende/)
  assert.match(msg(erroDoHub(new Error('the request timed out'))), /demorou/)
  // Código desconhecido: a frase do Hub, como veio.
  assert.deepEqual(erroDoHub(new Error('mail_outro: dê o login do provedor.')), { texto: 'Dê o login do provedor.' })
  assert.match(msg(erroDoHub('caiu a rede')), /Não deu para falar/)
})

test('parseAddressesInput separa por vírgula, ponto e vírgula e quebra de linha, e limpa vazios', () => {
  assert.deepEqual(parseAddressesInput('a@loja.example.invalid, b@loja.example.invalid; c@loja.example.invalid\nd@loja.example.invalid'),
    ['a@loja.example.invalid', 'b@loja.example.invalid', 'c@loja.example.invalid', 'd@loja.example.invalid'])
  assert.deepEqual(parseAddressesInput('  a@loja.example.invalid  , ,;\n\n'), ['a@loja.example.invalid'])
  assert.deepEqual(parseAddressesInput(', ; \n'), [])
  assert.deepEqual(parseAddressesInput(''), [])
})

test('a prévia: duas caixas fictícias em domínio reservado, e faltantes sem maiúsculas', () => {
  assert.deepEqual(CAIXAS_PREVIA.map((c) => c.endereco), ['suporte@loja.example.invalid', 'operacoes@loja.example.invalid'])
  assert.deepEqual(caixasPreviaFaltantes(null).map((c) => c.chave), ['suporte', 'operacoes'])
  const comSuporte = leitura({ mailboxes: [caixa({ addresses: ['SUPORTE@loja.example.invalid'] })] })
  assert.deepEqual(caixasPreviaFaltantes(comSuporte).map((c) => c.chave), ['operacoes'])
  const comAmbas = leitura({ mailboxes: [caixa(), caixa({ id: 'c2', label: 'Operações', addresses: ['operacoes@loja.example.invalid'] })] })
  assert.deepEqual(caixasPreviaFaltantes(comAmbas), [])
  // Endereço diferente não conta como conectado.
  assert.deepEqual(caixasPreviaFaltantes(leitura({ mailboxes: [caixa({ addresses: ['outra@loja.example.invalid'] })] })).length, 2)
})

test('permissões do bot: sem concessão nada, com concessão o que ela diz; contagem por bot distinto', () => {
  assert.deepEqual(permissoesDoBot(caixa(), 'donna'), { view: false, send: false, delete: false })
  const comGrants = caixa({ grants: [
    { participantKind: 'bot', participantId: 'donna', view: true, send: false, delete: true },
    { participantKind: 'bot', participantId: 'lia', view: false, send: false, delete: false },
    { participantKind: 'person', participantId: 'p1', view: true, send: true, delete: true },
  ] })
  assert.deepEqual(permissoesDoBot(comGrants, 'donna'), { view: true, send: false, delete: true })
  assert.deepEqual(permissoesDoBot(comGrants, 'lia'), { view: false, send: false, delete: false })
  // Só quem tem ao menos uma permissão ativa conta, uma vez por bot; pessoa não entra na contagem.
  assert.equal(contagemAutorizados(caixa()), 0)
  assert.equal(contagemAutorizados(comGrants), 1)
  const duplicado = caixa({ grants: [
    { participantKind: 'bot', participantId: 'donna', view: true, send: false, delete: false },
    { participantKind: 'bot', participantId: 'donna', view: false, send: true, delete: false },
    { participantKind: 'bot', participantId: 'lia', view: true, send: true, delete: true },
  ] })
  assert.equal(contagemAutorizados(duplicado), 2)
})

test('ler: a leitura chega; falha é erro, nunca lista vazia', async () => {
  let falhar = true
  const chamador: Chamador = { call: async <T>(method: string) => {
    if (method !== 'donnaMailManager') throw new Error('unknown method')
    if (falhar) throw new Error('mail_denied: recusado.')
    return leitura() as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  await estado.ler()
  assert.equal(estado.leitura, null)
  assert.match(msg(estado.erroLeitura!), /recusou o pedido/)
  falhar = false
  await estado.ler()
  assert.equal(estado.erroLeitura, null)
  assert.equal(atual(estado)?.bots.length, 2)
  assert.equal(estado.carregando, false)
  await espera()
})

test('sem conexão: mensagem clara, sem leitura', async () => {
  const estado = new EstadoMailManager(() => null)
  await estado.ler()
  assert.equal(estado.leitura, null)
  assert.match(msg(estado.erroLeitura!), /Sem conexão/)
  await estado.salvarPlugin(true)
  assert.match(msg(estado.erroPedido!), /Sem conexão/)
})

test('resposta antiga não sobrescreve a leitura mais nova', async () => {
  const pendentes: ((v: unknown) => void)[] = []
  const chamador: Chamador = { call: <T>() => new Promise<T>((r) => pendentes.push(r as (v: unknown) => void)) }
  const estado = new EstadoMailManager(() => chamador)
  const primeira = estado.ler()
  const segunda = estado.ler()
  pendentes[1]!(leitura())
  await segunda
  pendentes[0]!(leitura({ plugin: { enabled: false, mode: 'preview' } }))
  await primeira
  assert.equal(estado.leitura?.plugin.enabled, true)
  // O mesmo vale para um pedido: a resposta antiga do ler não volta por cima do que o pedido gravou.
  const lerAntiga = estado.ler()
  const salvar = estado.salvarPlugin(false)
  pendentes[3]!(leitura({ plugin: { enabled: false, mode: 'preview' } }))
  await salvar
  pendentes[2]!(leitura({ plugin: { enabled: true, mode: 'preview' } }))
  await lerAntiga
  assert.equal(estado.leitura?.plugin.enabled, false)
  await espera()
})

test('ligar e desligar: o pedido certo e a leitura da resposta', async () => {
  const pedidos: [string, unknown][] = []
  let ligado = true
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown) => {
    pedidos.push([method, body])
    if (method === 'donnaMailManagerSave') ligado = (body as { enabled: boolean }).enabled
    return leitura({ plugin: { enabled: ligado, mode: 'preview' } }) as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  await estado.ler()
  await estado.salvarPlugin(false)
  assert.deepEqual(pedidos[1], ['donnaMailManagerSave', { enabled: false }])
  assert.equal(estado.leitura?.plugin.enabled, false)
  await estado.salvarPlugin(true)
  assert.deepEqual(pedidos[2], ['donnaMailManagerSave', { enabled: true }])
  assert.equal(estado.leitura?.plugin.enabled, true)
  assert.equal(estado.salvando, false)
})

test('conectar e desconectar: o corpo do pedido, com endereços já limpos', async () => {
  const pedidos: [string, unknown][] = []
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown) => {
    pedidos.push([method, body])
    if (method === 'donnaMailManagerDisconnect') return leitura({ mailboxes: [] }) as T
    return { ...leitura({ mailboxes: [caixa()] }), connectedMailboxId: 'c1' } as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  await estado.conectar('Suporte', parseAddressesInput(' suporte@loja.example.invalid, '))
  assert.deepEqual(pedidos[0], ['donnaMailManagerConnect', { label: 'Suporte', addresses: ['suporte@loja.example.invalid'] }])
  assert.equal(estado.leitura?.mailboxes.length, 1)
  await estado.desconectar('c1')
  assert.deepEqual(pedidos[1], ['donnaMailManagerDisconnect', { mailboxId: 'c1' }])
  assert.equal(estado.leitura?.mailboxes.length, 0)
  await espera()
})

test('conectar: true só quando o pedido completa sem erro e a resposta é a vigente', async () => {
  let falhar = false
  const chamador: Chamador = { call: async <T>(method: string) => {
    if (method !== 'donnaMailManagerConnect') return leitura() as T
    if (falhar) throw new Error('mail_denied: recusado.')
    return { ...leitura({ mailboxes: [caixa()] }), connectedMailboxId: 'c1' } as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  assert.equal(await estado.conectar('Suporte', ['suporte@loja.example.invalid']), true)
  falhar = true
  assert.equal(await estado.conectar('Suporte', ['suporte@loja.example.invalid']), false)
  assert.match(msg(estado.erroPedido!), /recusou o pedido/)
  // Sem cliente: o pedido é ignorado com erro na tela.
  const semConexao = new EstadoMailManager(() => null)
  assert.equal(await semConexao.conectar('Suporte', ['suporte@loja.example.invalid']), false)
  assert.match(msg(semConexao.erroPedido!), /Sem conexão/)
  // Outro pedido em andamento: o segundo é ignorado, sem erro na tela.
  const pendentes: ((v: unknown) => void)[] = []
  const lento: Chamador = { call: <T>() => new Promise<T>((r) => pendentes.push(r as (v: unknown) => void)) }
  const ocupado = new EstadoMailManager(() => lento)
  const primeiro = ocupado.conectar('Suporte', ['suporte@loja.example.invalid'])
  assert.equal(await ocupado.conectar('Suporte', ['suporte@loja.example.invalid']), false)
  assert.equal(ocupado.erroPedido, null)
  pendentes[0]!({ ...leitura({ mailboxes: [caixa()] }), connectedMailboxId: 'c1' })
  assert.equal(await primeiro, true)
  // Resposta antiga (um ler mais novo começou antes dela voltar): completa sem erro, mas é ignorada.
  const antigo = ocupado.conectar('Suporte', ['suporte@loja.example.invalid'])
  const lerNovo = ocupado.ler()
  pendentes[1]!({ ...leitura({ mailboxes: [caixa()] }), connectedMailboxId: 'c1' })
  assert.equal(await antigo, false)
  pendentes[2]!(leitura())
  await lerNovo
  await espera()
})

test('conceder e revogar Ver, Enviar e Excluir para múltiplos bots na mesma caixa', async () => {
  let atual = leitura({ mailboxes: [caixa()] })
  const pedidos: [string, unknown][] = []
  const aplicar = (botId: string, permissao: PermissaoMail, valor: boolean): LeituraMailManager => ({
    ...atual,
    mailboxes: atual.mailboxes.map((c) => c.id !== 'c1' ? c : {
      ...c,
      grants: [
        ...c.grants.filter((g) => !(g.participantKind === 'bot' && g.participantId === botId)),
        { participantKind: 'bot' as const, participantId: botId, ...permissoesDoBot(c, botId), [permissao]: valor },
      ],
    }),
  })
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown) => {
    pedidos.push([method, body])
    if (method === 'donnaMailManagerGrant') {
      const { participantId, permission, allowed } = body as { participantId: string; permission: PermissaoMail; allowed: boolean }
      atual = aplicar(participantId, permission, allowed)
    }
    return atual as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  await estado.ler()
  assert.deepEqual(permissoesDoBot(estado.leitura!.mailboxes[0]!, 'donna'), { view: false, send: false, delete: false })
  await estado.alterarPermissao('c1', 'donna', 'view', true)
  await estado.alterarPermissao('c1', 'donna', 'send', true)
  await estado.alterarPermissao('c1', 'lia', 'delete', true)
  assert.deepEqual(pedidos.slice(1).map((p) => p[1]), [
    { mailboxId: 'c1', participantKind: 'bot', participantId: 'donna', permission: 'view', allowed: true },
    { mailboxId: 'c1', participantKind: 'bot', participantId: 'donna', permission: 'send', allowed: true },
    { mailboxId: 'c1', participantKind: 'bot', participantId: 'lia', permission: 'delete', allowed: true },
  ])
  // Os dois bots autorizados na mesma caixa, sem exigir bot gerente.
  assert.equal(contagemAutorizados(estado.leitura!.mailboxes[0]!), 2)
  assert.deepEqual(permissoesDoBot(estado.leitura!.mailboxes[0]!, 'donna'), { view: true, send: true, delete: false })
  // Revogar volta a concessão do valor anterior, sem mexer nas outras.
  await estado.alterarPermissao('c1', 'donna', 'view', false)
  assert.deepEqual(permissoesDoBot(estado.leitura!.mailboxes[0]!, 'donna'), { view: false, send: true, delete: false })
  assert.equal(contagemAutorizados(estado.leitura!.mailboxes[0]!), 2)
  await espera()
})

test('carregar prévia: conecta só as caixas fictícias que faltam, em sequência, sem conceder acesso', async () => {
  const pedidos: [string, unknown][] = []
  const conectadas: string[] = []
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown) => {
    pedidos.push([method, body])
    if (method === 'donnaMailManagerConnect') {
      const { addresses } = body as { addresses: string[] }
      conectadas.push(...addresses)
      return leitura({ mailboxes: conectadas.map((a, i) => ({ id: `c${i}`, label: a.split('@')[0]!, addresses: [a], grants: [] })) }) as T
    }
    return leitura({ mailboxes: conectadas.map((a, i) => ({ id: `c${i}`, label: a.split('@')[0]!, addresses: [a], grants: [] })) }) as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  // Sem leitura: as duas caixas faltam.
  await estado.carregarPrevia({ suporte: 'Suporte', operacoes: 'Operações' })
  assert.deepEqual(pedidos.map((p) => p[0]), ['donnaMailManagerConnect', 'donnaMailManagerConnect'])
  assert.deepEqual(pedidos.map((p) => p[1]), [
    { label: 'Suporte', addresses: ['suporte@loja.example.invalid'] },
    { label: 'Operações', addresses: ['operacoes@loja.example.invalid'] },
  ])
  assert.ok(!pedidos.some((p) => p[0] === 'donnaMailManagerGrant'), 'a prévia não concede acesso a nenhum bot')
  assert.equal(estado.leitura?.mailboxes.length, 2)
  assert.equal(estado.erroPedido, null)
  // Suporte já conectado (em maiúsculas): só a que falta entra.
  pedidos.length = 0
  conectadas.length = 0
  const estado2 = new EstadoMailManager(() => chamador)
  conectadas.push('suporte@loja.example.invalid')
  await estado2.ler()
  await estado2.carregarPrevia({ suporte: 'Suporte', operacoes: 'Operações' })
  assert.deepEqual(pedidos.filter((p) => p[0] === 'donnaMailManagerConnect').map((p) => p[1]), [
    { label: 'Operações', addresses: ['operacoes@loja.example.invalid'] },
  ])
  // As duas conectadas: nenhum pedido.
  pedidos.length = 0
  conectadas.push('operacoes@loja.example.invalid')
  await estado2.carregarPrevia({ suporte: 'Suporte', operacoes: 'Operações' })
  assert.deepEqual(pedidos, [])
  await espera()
})

test('carregar prévia: uma falha para a sequência e fica na tela como erro do pedido', async () => {
  let chamadas = 0
  const chamador: Chamador = { call: async <T>(method: string) => {
    if (method === 'donnaMailManagerConnect') {
      chamadas++
      if (chamadas === 1) return leitura({ mailboxes: [caixa()] }) as T
      throw new Error('mail_denied: recusado.')
    }
    return leitura() as T
  } }
  const estado = new EstadoMailManager(() => chamador)
  await estado.carregarPrevia({ suporte: 'Suporte', operacoes: 'Operações' })
  assert.equal(chamadas, 2)
  assert.match(msg(estado.erroPedido!), /recusou o pedido/)
  await espera()
})
