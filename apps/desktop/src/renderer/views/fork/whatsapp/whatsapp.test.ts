import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { EstadoWhatsapp, type Chamador } from './estado.ts'
import type { ItemFila } from './fila.ts'
import { tradutor, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { textoDaMensagem, type Mensagem } from '../bot-settings/mensagem.ts'
import { acoesDoNumero, avisoWacli, computadorPessoal, numeroNoComputador, relerEmMs, bloqueioConectar, bloqueioDoBot, clamp, codigoFormatado, conferirLeitura, erroDoHub, ESPERA, horaDeMinutos, LIMITE, LIMITE_DIA, LIMITE_DIA_PADRAO, limiteDiaDe, minutosDeHora, numeroTexto, oQueOBotPode, oQuePode, patchVer, quatroEstados, silencioDe, SILENCIO_PADRAO, telefoneParece, type AjustesBot, type LeituraWhatsapp } from './regras.ts'

// D70: só as regras que a tela repete do Hub e o estado de leitura/erro. Fatia 6 (D454): as regras devolvem chaves
// e mensagens como dado; o teste confere o texto pt-BR que a tela mostra (`tx`/`msg`) e o inglês onde importa.
const t = tradutor('pt-BR')
const semParametros = (tr: typeof t) => tr as unknown as (chave: ChaveTexto) => string
const tx = (k: ChaveTexto) => semParametros(t)(k)
const msg = (m: Mensagem) => textoDaMensagem(t, m)

const ajustes = { readChats: false, replyChats: false, readGroups: false, replyGroups: false, replyMode: 'approval' as const, delaySeconds: 8, hourlyLimit: 30, transcribeAudio: true }
const nada = { readChats: false, replyChats: false, readGroups: false, replyGroups: false }
const leitura = (enabled: boolean, mode: 'donna_only' | 'per_bot' = 'donna_only'): LeituraWhatsapp => ({
  plugin: { enabled, mode },
  transcription: { provider: null, model: null, source: 'none', keyOnHub: false },
  bots: [
    { botId: 'donna', isDonna: true, paused: false, settings: ajustes, number: { state: 'none' }, botCan: nada, sending: { state: 'none' } },
    { botId: 'lia', isDonna: false, paused: mode === 'donna_only', settings: ajustes, number: { state: 'none' }, botCan: nada, sending: { state: 'none' } },
  ],
})
const atual = (e: EstadoWhatsapp) => e.leitura
const espera = () => new Promise((r) => setTimeout(r, 0))

test('desligar "Ver" desliga "Responder" no mesmo pedido', () => {
  assert.deepEqual(patchVer('chats', false), { readChats: false, replyChats: false })
  assert.deepEqual(patchVer('groups', false), { readGroups: false, replyGroups: false })
  assert.deepEqual(patchVer('chats', true), { readChats: true })
})

test('faixas de 3–60 s e 1–60/h', () => {
  assert.equal(clamp(3, ESPERA), 8)
  assert.equal(clamp(999, ESPERA), 300)
  assert.equal(clamp(0, LIMITE), 1)
  assert.equal(clamp(500, LIMITE), 200)
  assert.equal(clamp(Number.NaN, LIMITE), 1)
})

test('bot pausado no modo Só a Donna e plugin desligado ficam travados, com explicação', () => {
  const off = leitura(false)
  assert.match(tx(bloqueioDoBot(off, off.bots[0]!)!), /Ligue o WhatsApp/)
  const donna = leitura(true)
  assert.equal(bloqueioDoBot(donna, donna.bots[0]!), null)
  assert.match(tx(bloqueioDoBot(donna, donna.bots[1]!)!), /Pausado/)
  const porBot = leitura(true, 'per_bot')
  assert.equal(bloqueioDoBot(porBot, porBot.bots[1]!), null)
})

test('os 4 estados ficam separados', () => {
  const v = leitura(true)
  const e = quatroEstados(v, { ...v.bots[0]!, botCan: { ...nada, readChats: true, replyChats: true } })
  assert.deepEqual({ ligado: e.ligado, plugin: tx(e.plugin), numero: tx(e.numero), pode: tx(e.pode), envio: tx(e.envio) }, { ligado: true, plugin: 'Ligado', numero: 'Sem número', pode: 'Pode ver e responder conversas · responde só com sua aprovação', envio: 'Espera seu ok' })
  // MX-18: no modo "sozinho" o estado 4 diz que sai sem cartão.
  const sozinho = { ...v.bots[0]!, settings: { ...v.bots[0]!.settings, replyMode: 'autonomous' as const } }
  assert.equal(tx(quatroEstados(v, sozinho).envio), 'Responde sozinho')
  assert.equal(tx(quatroEstados(v, v.bots[1]!).pode), 'Pausado')
  assert.equal(tx(oQuePode(nada)), 'Não pode nada')
  // A frase inteira vem do catálogo, sem juntar pedaços: em inglês, a mesma combinação.
  assert.equal(semParametros(tradutor('en'))(oQuePode({ ...nada, readChats: true, readGroups: true, replyGroups: true })), 'Can see chats · see and reply to groups · replies only with your approval')
})

test('erros do Hub viram frase clara', () => {
  assert.equal(msg(erroDoHub(new Error('plugin_disabled: ligue o plugin WhatsApp primeiro.'))), 'Ligue o plugin WhatsApp primeiro.')
  assert.match(msg(erroDoHub({ status: 404, message: 'unknown method' })), /ainda não tem o WhatsApp/)
  assert.match(msg(erroDoHub(new Error('not_allowed: só o dono pode usar o editor.'))), /Só o dono/)
  assert.throws(() => conferirLeitura({ plugin: { enabled: true, mode: 'x' }, bots: [] }), /resposta_invalida/)
  assert.throws(() => conferirLeitura({}), /resposta_invalida/)
})

test('falha de leitura é erro, nunca lista vazia; falha de pedido mantém a leitura', async () => {
  let falhar = true
  const chamador: Chamador = { call: async <T>(method: string) => {
    if (falhar) throw new Error('unknown method')
    if (method === 'donnaWhatsappBotSave') throw new Error('not_allowed: no modo Só a Donna, só a Donna usa o WhatsApp.')
    return leitura(true) as T
  } }
  const estado = new EstadoWhatsapp(() => chamador)
  await estado.ler()
  assert.equal(estado.leitura, null as LeituraWhatsapp | null)
  assert.match(msg(estado.erroLeitura!), /Atualize/)
  falhar = false
  await estado.ler()
  assert.equal(estado.erroLeitura, null)
  assert.equal(atual(estado)?.bots.length, 2)
  await estado.salvarBot('lia', { readChats: true })
  assert.equal(msg(estado.erroPedido!), 'No modo Só a Donna, só a Donna usa o WhatsApp.')
  assert.equal(atual(estado)?.bots.length, 2)
  await espera()
})

test('resposta antiga não sobrescreve a leitura mais nova', async () => {
  const pendentes: ((v: unknown) => void)[] = []
  const chamador: Chamador = { call: <T>() => new Promise<T>((r) => pendentes.push(r as (v: unknown) => void)) }
  const estado = new EstadoWhatsapp(() => chamador)
  const primeira = estado.ler()
  const segunda = estado.ler()
  pendentes[1]!(leitura(true, 'per_bot'))
  await segunda
  pendentes[0]!(leitura(false))
  await primeira
  assert.equal(estado.leitura?.plugin.mode, 'per_bot')
  assert.equal(estado.leitura?.plugin.enabled, true)
  await espera()
})

test('sem conexão: mensagem clara', async () => {
  const estado = new EstadoWhatsapp(() => null)
  await estado.ler()
  assert.match(msg(estado.erroLeitura!), /Sem conexão/)
})

// R5b: o número (R2).

test('estados do número viram texto e ações certas', () => {
  assert.equal(tx(numeroTexto({ state: 'connected' })), 'Conectado')
  assert.equal(tx(numeroTexto({ state: 'needs_reconnect' })), 'Precisa conectar de novo')
  assert.equal(tx(numeroTexto({ state: 'not_installed' })), 'wacli não instalado')
  assert.equal(tx(numeroTexto({ state: 'algo_novo' })), 'Estado do número desconhecido')
  assert.deepEqual(acoesDoNumero({ state: 'none' }), { conectar: true, rotuloConectar: 'whatsapp.estados.acao.conectar', desconectar: false })
  assert.deepEqual(acoesDoNumero({ state: 'needs_reconnect' }), { conectar: true, rotuloConectar: 'whatsapp.estados.acao.conectarDeNovo', desconectar: true })
  assert.equal(acoesDoNumero({ state: 'connected' }).conectar, false)
  assert.equal(acoesDoNumero({ state: 'stopped' }).desconectar, true)
  assert.equal(acoesDoNumero({ state: 'not_installed' }).conectar, false)
})

test('conectar trava com plugin desligado, bot pausado ou sem wacli', () => {
  const v = leitura(true)
  assert.equal(bloqueioConectar(v, v.bots[0]!), null)
  assert.match(tx(bloqueioConectar(v, v.bots[1]!)!), /Pausado/)
  assert.match(tx(bloqueioConectar(leitura(false), v.bots[0]!)!), /Ligue o WhatsApp/)
  const sem = { ...v, plugin: { ...v.plugin, wacli: { installed: false } } }
  assert.match(tx(bloqueioConectar(sem, v.bots[0]!)!), /wacli/)
})

test('código em 4+4 e telefone com DDD', () => {
  assert.equal(codigoFormatado('abcdefgh'), 'ABCD-EFGH')
  assert.equal(codigoFormatado('ABCD-EFGH'), 'ABCD-EFGH')
  assert.equal(telefoneParece('+55 11 90000-0002'), true)
  assert.equal(telefoneParece('9000'), false)
})

test('pareamento espera mais que o padrão e o aviso de logout que falhou não se perde', async () => {
  const pedidos: [string, unknown, number | undefined][] = []
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown, timeoutMs?: number) => {
    pedidos.push([method, body, timeoutMs])
    const v = leitura(true) as unknown as Record<string, unknown>
    if (method === 'donnaWhatsappDisconnect') return { ...v, disconnect: { logout: 'failed', warning: 'Tire o aparelho no celular.' } } as T
    return v as T
  } }
  const estado = new EstadoWhatsapp(() => chamador)
  await estado.parear('donna', 'code', '+55 11 90000-0002')
  assert.deepEqual(pedidos[0]!.slice(0, 2), ['donnaWhatsappPair', { botId: 'donna', method: 'code', phone: '+55 11 90000-0002' }])
  assert.ok(pedidos[0]![2]! > 20_000, 'o Hub segura o pedido até 20 s pelo primeiro código')
  await estado.parear('donna', 'qr')
  assert.deepEqual(pedidos[1]![1], { botId: 'donna', method: 'qr' })
  const desconectar = estado.desconectar('donna', true)
  void estado.ler() // um evento do Hub relê antes da resposta
  await desconectar
  assert.deepEqual(estado.avisoLogout, { botId: 'donna', mensagem: { texto: 'Tire o aparelho no celular.' } })
  assert.deepEqual(pedidos.find((p) => p[0] === 'donnaWhatsappDisconnect')![1], { botId: 'donna', erase: true })
  await espera()
})

// R7: o download do wacli e o computador do número.

test('estado do wacli vira aviso claro, com Tentar de novo só na falha', () => {
  assert.equal(avisoWacli(t, { installed: false, state: 'downloading' })!.titulo, 'Baixando o wacli…')
  assert.equal(avisoWacli(t, { installed: true, state: 'ready', version: 'v0.25.1' })!.tom, 'ok')
  const falha = avisoWacli(t, { installed: false, state: 'failed', reason: 'wacli_integrity: o pacote baixado não confere.' })!
  assert.deepEqual([falha.titulo, falha.detalhe, falha.tentar], ['O wacli não foi instalado', 'O pacote baixado não confere.', true])
  assert.equal(avisoWacli(t, { installed: false, state: 'unsupported' })!.titulo, 'Sem wacli oficial para este computador')
  assert.equal(avisoWacli(t, { installed: false, state: 'off' }), null)
  assert.equal(avisoWacli(t, { installed: false })!.titulo, 'wacli não instalado', 'Hub do R2, sem state')
})

test('conectar explica o wacli; a tela relê só enquanto baixa', () => {
  const v = leitura(true)
  const com = (wacli: LeituraWhatsapp['plugin']['wacli']) => ({ ...v, plugin: { ...v.plugin, wacli } })
  assert.match(tx(bloqueioConectar(com({ installed: false, state: 'downloading' }), v.bots[0]!)!), /terminar de baixar/)
  assert.match(tx(bloqueioConectar(com({ installed: false, state: 'unsupported' }), v.bots[0]!)!), /Sem wacli oficial/)
  assert.equal(bloqueioConectar(com({ installed: true, state: 'ready' }), v.bots[0]!), null)
  assert.equal(relerEmMs(com({ installed: false, state: 'downloading' })), 3000)
  assert.equal(relerEmMs(com({ installed: true, state: 'ready' })), null)
  assert.equal(relerEmMs({ ...com({ installed: false, state: 'downloading' }), plugin: { enabled: false, mode: 'per_bot', wacli: { installed: false, state: 'downloading' } } }), null)
})

test('"só com o computador ligado" só em Mac ou PC, e só com número no computador', () => {
  assert.equal(computadorPessoal('macos'), true)
  assert.equal(computadorPessoal('windows'), true)
  assert.equal(computadorPessoal('linux'), false)
  assert.equal(computadorPessoal(undefined), false)
  assert.equal(numeroNoComputador({ state: 'connected' }), true)
  assert.equal(numeroNoComputador({ state: 'none' }), false)
})

test('Tentar de novo chama o RPC do R7 sem campos', async () => {
  const pedidos: [string, unknown][] = []
  const estado = new EstadoWhatsapp(() => ({ call: async <T>(m: string, b?: unknown) => { pedidos.push([m, b]); return leitura(true) as T } }))
  await estado.tentarWacli()
  assert.deepEqual(pedidos, [['donnaWhatsappWacliRetry', {}]])
  await espera()
})

// Aviso do R3 (#321): botCan vem da concessão e fica falso sem número, mesmo com a opção ligada.

test('opção ligada sem número pede para conectar, antes e depois do R3', () => {
  const base = leitura(true, 'per_bot').bots[0]!
  const ligada = { ...base, settings: { ...base.settings, readChats: true } }
  // Depois do #321: botCan falso sem número.
  assert.equal(tx(oQueOBotPode({ ...ligada, botCan: nada })), 'Conecte o número para o bot ver')
  // Antes do #321: botCan verdadeiro mesmo sem número; a tela não promete o que não chega.
  assert.equal(tx(oQueOBotPode({ ...ligada, botCan: { ...nada, readChats: true } })), 'Conecte o número para o bot ver')
  assert.equal(tx(oQueOBotPode({ ...ligada, number: { state: 'needs_reconnect' }, botCan: nada })), 'Conecte o número de novo para o bot ver')
  assert.equal(tx(oQueOBotPode({ ...ligada, number: { state: 'connected' }, botCan: { ...nada, readChats: true } })), 'Pode ver conversas')
  assert.equal(tx(oQueOBotPode({ ...base, botCan: nada })), 'Não pode nada', 'tudo desligado continua "Não pode nada"')
  assert.equal(tx(oQueOBotPode({ ...ligada, paused: true })), 'Pausado')
  assert.equal(tx(quatroEstados(leitura(true), { ...ligada, botCan: nada }).pode), 'Conecte o número para o bot ver')
})

test('leitura com inbox (R3) continua válida', () => {
  const v = { ...leitura(true), inbox: { unread: 3 } }
  assert.equal(conferirLeitura(v).bots.length, 2)
})

// MX-18: limite por dia, horário de silêncio e a fila de saída.

test('limite por dia e silêncio: padrão do dono em Hub antigo, ajuste ida e volta', () => {
  const antigo = ajustes as AjustesBot
  assert.equal(LIMITE_DIA_PADRAO, 100)
  assert.equal(limiteDiaDe(antigo), 100)
  assert.deepEqual(silencioDe(antigo), { on: true, start: SILENCIO_PADRAO.start, end: SILENCIO_PADRAO.end })
  assert.deepEqual(SILENCIO_PADRAO, { start: 1320, end: 480 })
  const ajustado: AjustesBot = { ...antigo, dailyLimit: 40, quietEnabled: false, quietStart: 23 * 60, quietEnd: 7 * 60 }
  assert.equal(limiteDiaDe(ajustado), 40)
  assert.deepEqual(silencioDe(ajustado), { on: false, start: 1380, end: 420 })
  assert.equal(clamp(limiteDiaDe(ajustado) + 1, LIMITE_DIA), 41)
  assert.equal(clamp(0, LIMITE_DIA), 1)
  assert.equal(clamp(999, LIMITE_DIA), 500)
  // "22:00" ↔ 1320 e volta; texto inválido não vira horário.
  assert.equal(horaDeMinutos(1320), '22:00')
  assert.equal(horaDeMinutos(480), '08:00')
  assert.equal(minutosDeHora('22:00'), 1320)
  assert.equal(minutosDeHora('8:05'), 485)
  assert.equal(minutosDeHora('24:00'), null)
  assert.equal(minutosDeHora('abc'), null)
  assert.equal(minutosDeHora(''), null)
})

test('modo "sozinho": a frase do que o bot pode diz que responde sem cartão, só nas marcadas', () => {
  const rr = { ...nada, readChats: true, replyChats: true, readGroups: true, replyGroups: true }
  assert.equal(semParametros(t)(oQuePode(rr)), 'Pode ver e responder conversas · ver e responder grupos · responde só com sua aprovação')
  assert.equal(semParametros(t)(oQuePode(rr, true)), 'Pode ver e responder conversas · ver e responder grupos · responde sozinho nas marcadas')
  assert.equal(semParametros(tradutor('en'))(oQuePode({ ...nada, readChats: true, replyChats: true }, true)), 'Can view and reply to chats · replies on its own in marked ones')
  // Sem "Responder" ligado, o modo não muda a frase.
  assert.equal(semParametros(t)(oQuePode({ ...nada, readChats: true }, true)), 'Pode ver conversas')
})

test('fila de saída: leitura vira itens; formato errado é erro; decisão atualiza com a resposta do Hub', async () => {
  const itens: ItemFila[] = [{ id: 'q1', botId: 'donna', chatId: '5511900000102@s.whatsapp.net', isGroup: false, replyTo: 'm1', text: 'oi', origin: 'owner_card', state: 'uncertain', reason: 'uncertain', notBefore: 0, attempts: 1, sentAt: null, createdAt: 1 }]
  const pedidos: [string, unknown][] = []
  let fila = { items: itens }
  const chamador: Chamador = { call: async <T>(method: string, body?: unknown) => {
    pedidos.push([method, body])
    if (method === 'donnaWhatsappQueue') return fila as T
    if (method === 'donnaWhatsappQueueDecide') {
      fila = { items: [{ ...itens[0]!, state: 'sent', reason: null, sentAt: 2 }] }
      return fila as T
    }
    throw new Error('unknown method')
  } }
  const estado = new EstadoWhatsapp(() => chamador)
  await estado.lerFila('donna')
  assert.deepEqual(pedidos[0], ['donnaWhatsappQueue', { botId: 'donna' }])
  assert.equal(estado.filas.get('donna')?.items?.[0]?.state, 'uncertain')
  assert.equal(estado.filas.get('donna')?.erro, null)
  await estado.decidirFila('donna', 'q1', 'sent')
  assert.deepEqual(pedidos[1], ['donnaWhatsappQueueDecide', { botId: 'donna', itemId: 'q1', decision: 'sent' }])
  assert.equal(estado.filas.get('donna')?.items?.[0]?.state, 'sent')
  // Formato errado: erro na tela, nunca lista vazia.
  fila = { items: 'quebrado' } as never
  await estado.lerFila('donna')
  assert.equal(estado.filas.get('donna')?.items, null)
  assert.match(msg(estado.filas.get('donna')?.erro!), /não entende/)
  await espera()
})

test('R1-02 do #587: carregando, erro e a seção ficam na superfície do idioma (todos os retornos)', () => {
  const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
  assert.match(fonte('./SecaoWhatsapp.tsx'), /export function SecaoWhatsapp\(props[^)]*\) \{\n  return <Superficie><ConteudoWhatsapp \{\.\.\.props\} \/><\/Superficie>/)
  const plugins = fonte('./PluginsPage.tsx')
  const raiz = plugins.slice(plugins.indexOf('export function PluginsPage'), plugins.indexOf('function WhatsappPlugin'))
  assert.match(raiz, /<Superficie>[\s\S]*<WhatsappPlugin[\s\S]*semComputador[\s\S]*<\/Superficie>/)
})
