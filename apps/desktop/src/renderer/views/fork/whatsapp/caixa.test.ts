import assert from 'node:assert/strict'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { textoDaMensagem } from '../bot-settings/mensagem.ts'
import test from 'node:test'
import { ALVO_AO_ABRIR, seletorDaLinha, contagens, mostraAba, transcricao, duracao, estadoNumero, filtrar, formatarTelefone, iconeDa, iniciais, itensConversa, nomeDa, previa, temNumero, verLigado, type ConversaWa, type MensagemWa } from './caixa.ts'
import { CaixaWhatsapp, caixaDe } from './caixa-estado.ts'
import { estadoDe } from './estado.ts'
import { origemDoVoo, podeFocar, seguraTecla, recuo, SELETOR_FOCO, teclaTelaGrande, tempoDoToken, tempoDoVoo, transformeMoldura } from './tela-grande.ts'
import { PANEL_EASE } from '../painel-direito/curva.ts'
import { readFileSync } from 'node:fs'
import { traduzir, traduzirPlural } from '../../../lib/fork-i18n/index.ts'
import { prazoDe, PRAZOS, type AjustesBot, type BotWhatsapp } from './regras.ts'

const last = (o: Partial<ConversaWa['last']> = {}): ConversaWa['last'] => ({ senderName: 'Paulo', senderContact: true, body: 'oi', media: null, call: null, sentAt: 1, offline: false, ...o })
const conv = (o: Partial<ConversaWa>): ConversaWa => ({ whatsappChatId: 'x', kind: 'individual', name: 'Carlos', phone: '+5511900000102', pinned: false, unread: 0, last: last(), ...o })
const lista = [
  conv({ whatsappChatId: 'g1', kind: 'group', name: 'Fornecedores', phone: null, pinned: true, last: last({ senderContact: false, body: 'Entrega sexta' }) }),
  conv({ whatsappChatId: 'c', unread: 2, last: last({ body: 'Pode mandar o orçamento?' }) }),
  conv({ whatsappChatId: 'g2', kind: 'group', name: 'Clientes VIP', phone: null, unread: 1 }),
  conv({ whatsappChatId: 'n', name: null, phone: '+5511900000107', unread: 1 }),
  conv({ whatsappChatId: 'b', name: 'Beatriz', last: last({ body: 'Mensagem de voz', media: 'audio' }) }),
]

test('lista: só o nome; o número só sem nome; prévia de grupo com ~Nome do remetente não salvo', () => {
  assert.equal(nomeDa(lista[1]!), 'Carlos')
  assert.equal(nomeDa(lista[3]!), '+55 11 90000-0107')
  assert.equal(previa(lista[0]!), '~Paulo: Entrega sexta')
  assert.equal(previa(conv({ kind: 'group', last: last({ senderName: 'Ana', senderContact: true, body: 'ok' }) })), 'Ana: ok')
  assert.equal(previa(lista[1]!), 'Pode mandar o orçamento?')
  assert.equal(iniciais({ name: 'Carlos (exemplo)' }), 'C')
  assert.equal(iniciais({ name: null }), '')
  assert.equal(formatarTelefone('+12025550100'), '+12025550100')
})

test('lista: mensagem de voz sem duração; chamada com ícone e duração quando o wacli informa', () => {
  assert.equal(previa(lista[4]!), 'Mensagem de voz')
  assert.equal(iconeDa(lista[4]!.last), 'mic')
  assert.equal(iconeDa({ media: null, call: { media: 'video' } }), 'video')
  assert.equal(iconeDa({ media: null, call: { media: 'audio' } }), 'phone')
  assert.equal(iconeDa({ media: 'sticker', call: null }), 'face.smiling')
  assert.equal(duracao(125), '2:05')
  assert.equal(duracao(null), null)
  assert.equal(duracao(0), null)
})

test('filtros e contagens: Não lidas = conversas com não lidas, Privadas, Grupos; busca por nome e número; a ordem do Hub fica', () => {
  assert.deepEqual(contagens(lista), { naoLidas: 3, privadas: 3, grupos: 2 })
  assert.deepEqual(filtrar(lista, 'todas', '').map((c) => c.whatsappChatId), ['g1', 'c', 'g2', 'n', 'b'])
  assert.deepEqual(filtrar(lista, 'naoLidas', '').map((c) => c.whatsappChatId), ['c', 'g2', 'n'])
  assert.deepEqual(filtrar(lista, 'privadas', '').map((c) => c.whatsappChatId), ['c', 'n', 'b'])
  assert.deepEqual(filtrar(lista, 'grupos', '').map((c) => c.whatsappChatId), ['g1', 'g2'])
  assert.deepEqual(filtrar(lista, 'todas', 'fornec').map((c) => c.whatsappChatId), ['g1'])
  assert.deepEqual(filtrar(lista, 'todas', '0107').map((c) => c.whatsappChatId), ['n'])
  assert.deepEqual(filtrar(lista, 'grupos', 'carlos'), [])
})

const bot = (state: string, s: Partial<BotWhatsapp['settings']> = {}, phone?: string): BotWhatsapp => ({
  botId: 'b', isDonna: false, paused: false, number: { state, ...(phone ? { phone } : {}) }, sending: { state: 'none' },
  botCan: { readChats: false, replyChats: false, readGroups: false, replyGroups: false },
  settings: { readChats: false, replyChats: false, readGroups: false, replyGroups: false, replyMode: 'approval', delaySeconds: 8, hourlyLimit: 30, transcribeAudio: true, ...s },
})

test('estados: a aba só com número; Ver desligado; desconectado com faixa', () => {
  assert.equal(temNumero(bot('none')), false)
  assert.equal(temNumero(bot('pairing')), false)
  assert.equal(temNumero(null), false)
  assert.equal(temNumero(bot('connected')), true)
  assert.equal(temNumero(bot('disconnected')), true)
  // D350: plugin desligado = sem aba, mesmo com número (o Hub diz "parado").
  assert.equal(mostraAba(false, bot('stopped')), false)
  assert.equal(mostraAba(false, bot('connected')), false)
  assert.equal(mostraAba(true, bot('connected')), true)
  assert.equal(mostraAba(true, bot('none')), false)
  assert.equal(verLigado(bot('connected')), false)
  assert.equal(verLigado(bot('connected', { readGroups: true })), true)
  assert.deepEqual(estadoNumero(bot('connected', {}, '+5511900000101')), { texto: 'whatsapp.estados.numero.connected', phone: '+5511900000101', ruim: false, faixa: null })
  assert.equal(estadoNumero(bot('needs_reconnect')).faixa?.titulo, 'whatsapp.estados.faixa.desconectado.titulo')
})

const msg = (seq: number, sentAt: number, offline = false): MensagemWa => ({ seq, messageId: `m${seq}`, from: { name: 'C', phone: null, contact: true }, call: null, sentAt, body: 't', media: null, offline, read: true })

test('conversa: mais antiga em cima, dia, e "chegaram com o computador desligado" uma vez por sequência', () => {
  const now = new Date(2026, 10, 6, 12)
  const ontem = new Date(2026, 10, 5, 17).getTime()
  const hoje = (h: number) => new Date(2026, 10, 6, h).getTime()
  const itens = itensConversa([msg(4, hoje(10)), msg(3, hoje(8), true), msg(2, hoje(7), true), msg(1, ontem)], now)
  assert.deepEqual(itens.map((i) => (i.tipo === 'msg' ? i.m.seq : i.tipo === 'dia' ? i.texto : 'off')), ['Ontem', 1, 'Hoje', 'off', 2, 3, 4])
})

const chamador = (respostas: Record<string, (body: Record<string, unknown>) => unknown>) => {
  const chamadas: [string, unknown][] = []
  return { chamadas, call: async <T,>(m: string, b?: unknown) => { chamadas.push([m, b]); const f = respostas[m]; if (!f) throw new Error('unknown method'); return f(b as Record<string, unknown>) as T } }
}

test('caixa: erro do Hub fica como erro (nunca lista vazia); marcar como lida zera a bolinha; cada aviso relê uma vez', async () => {
  let falhar = true
  const c = chamador({
    donnaWhatsappChats: () => { if (falhar) throw new Error('timeout'); return { chats: [conv({ whatsappChatId: 'c', unread: 2 })], unread: 2 } },
    donnaWhatsappMarkRead: () => ({ unread: 0 }),
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  assert.equal(caixa.lista('b')?.conversas, null)
  const erro = caixa.lista('b')?.erro
  assert.match(erro ? textoDaMensagem(tradutor('pt-BR'), erro) : '', /demorou/)
  falhar = false
  await caixa.lerConversas('b')
  assert.equal(caixa.lista('b')?.conversas?.[0]?.unread, 2)
  await caixa.marcarLida('b', 'c')
  assert.deepEqual(c.chamadas.at(-1), ['donnaWhatsappMarkRead', { botId: 'b', whatsappChatId: 'c' }])
  assert.equal(caixa.lista('b')?.conversas?.[0]?.unread, 0)
  const antes = c.chamadas.length
  caixa.sincronizar({ seq: 5, botId: 'b' }) // montar: só anota
  caixa.sincronizar({ seq: 6, botId: 'b' })
  caixa.sincronizar({ seq: 6, botId: 'b' }) // a tela grande montada junto não relê de novo
  await new Promise((r) => setTimeout(r, 0))
  assert.deepEqual(c.chamadas.slice(antes).map(([m]) => m), ['donnaWhatsappChats'])
})

test('tela grande: a moldura só com transform; tokens de movimento; Esc e ⌘1', () => {
  assert.equal(transformeMoldura({ left: 1100, top: 8, width: 332, height: 884 }, { left: 300, top: 8, width: 1000, height: 884 }), 'translate(800px, 0px) scale(0.332, 1)')
  assert.deepEqual(recuo({ left: 0, top: 0, width: 100, height: 50 }, 8), { left: 8, top: 8, width: 84, height: 34 })
  assert.deepEqual(tempoDoToken(' 0.3s linear(0, 1)'), { ms: 300, curva: 'linear(0, 1)' })
  assert.deepEqual(tempoDoToken('0.12s cubic-bezier(0.25, 0.1, 0.25, 1)'), { ms: 120, curva: 'cubic-bezier(0.25, 0.1, 0.25, 1)' })
  // Movimento reduzido: o tema zera --layout e --fade (theme.css), e 0 ms = troca na hora.
  assert.equal(tempoDoToken('0s').ms, 0)
  const k = (key: string, o: Partial<{ metaKey: boolean; ctrlKey: boolean }> = {}) => ({ key, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...o })
  assert.equal(teclaTelaGrande(k('Escape'), true), 'recolher')
  assert.equal(teclaTelaGrande(k('1', { metaKey: true }), true), 'conversa')
  assert.equal(teclaTelaGrande(k('1', { ctrlKey: true }), false), 'conversa')
  assert.equal(teclaTelaGrande(k('1', { ctrlKey: true }), true), null)
  assert.equal(teclaTelaGrande(k('2', { metaKey: true }), true), null)
})

test('D350: a caixa usa a leitura compartilhada; ligar e desligar em Plugins muda a aba na hora', async () => {
  let enabled = true
  const leitura = () => ({ plugin: { enabled, mode: 'per_bot' }, transcription: { provider: null, model: null, source: 'none', keyOnHub: false },
    bots: [{ ...bot('connected'), botId: 'b' }] })
  const c = chamador({ donnaWhatsapp: leitura, donnaWhatsappSave: (b) => { enabled = (b as { enabled: boolean }).enabled; return leitura() } })
  const loja = {}
  const caixa = caixaDe(loja, () => c)
  const plugins = estadoDe(loja, () => c)
  assert.equal(caixa.leitura, plugins)
  await plugins.ler()
  assert.equal(mostraAba(caixa.leitura.leitura!.plugin.enabled, caixa.bot('b')), true)
  await plugins.salvarHub({ enabled: false })
  assert.equal(mostraAba(caixa.leitura.leitura!.plugin.enabled, caixa.bot('b')), false)
})

test('A1: invertendo no meio, o voo parte do retângulo ATUAL da moldura (sem salto)', () => {
  const atual = { left: 570.84, top: 8, width: 861.25, height: 884 }
  const cheio = { left: 346, top: 8, width: 1085.97, height: 884 }
  const painel = { left: 1100, top: 8, width: 332, height: 884 }
  const de = origemDoVoo(atual, cheio)!
  assert.deepEqual(de, atual)
  // O 1º quadro da volta põe a moldura exatamente onde ela estava.
  assert.equal(transformeMoldura(de, painel), transformeMoldura(atual, painel))
  assert.notEqual(transformeMoldura(de, painel), transformeMoldura(cheio, painel))
  // Sem voo em andamento, parte do lugar de sempre.
  assert.deepEqual(origemDoVoo(null, cheio), cheio)
})

test('A2: o foco vai ao controle de cada lado quando a troca termina, com e sem movimento reduzido', () => {
  assert.deepEqual(SELETOR_FOCO.tela, ['.wa-tela .wa-x-recolher'])
  assert.equal(SELETOR_FOCO.painel[0], '.rpanel .wa-expand')
  assert.equal(SELETOR_FOCO.painel[1], '.rpanel [role="tab"][aria-selected="true"]')
  assert.deepEqual(SELETOR_FOCO.conversa, ['.ctab.main'])
  // Com movimento: espera a moldura assentar e o véu sair.
  assert.equal(podeFocar('abrindo', false), false)
  assert.equal(podeFocar('fechando', true), false)
  assert.equal(podeFocar('parada', true), false)
  assert.equal(podeFocar('parada', false), true)
  // Movimento reduzido: --layout = 0 s, nada voa, a fase já nasce "parada" e o foco vai na hora.
  assert.equal(tempoDoVoo('0s').ms, 0)
  assert.equal(podeFocar('parada', false), true)
})

test('A4: o voo dura o --layout (0,3 s) com a curva do fixar do painel #352, um valor só', () => {
  assert.deepEqual(tempoDoVoo(' 0.3s linear(0, 0.0712, 1)'), { ms: 300, curva: 'cubic-bezier(0.32, 0.72, 0, 1)' })
  assert.equal(tempoDoVoo('0.3s linear(0, 1)').curva, PANEL_EASE)
  const painel = readFileSync(new URL('../painel-direito/PainelDireito.tsx', import.meta.url), 'utf8')
  assert.match(painel, /const ease = PANEL_EASE/)
  assert.doesNotMatch(painel, /cubic-bezier\(0\.32, 0\.72, 0, 1\)/)
})

test('A2 (rodada 2): durante o voo a moldura segura Tab e Shift+Tab; Esc e as outras teclas passam', () => {
  assert.equal(seguraTecla('Tab'), true)
  assert.equal(seguraTecla('Escape'), false)
  assert.equal(seguraTecla('1'), false)
  assert.equal(seguraTecla('Enter'), false)
})

test('voz (R3b): texto transcrito com quem transcreveu; os estados sem texto; nada sem transcrição pedida', () => {
  const voz = (transcript: MensagemWa['transcript']) => ({ media: 'audio', transcript })
  assert.deepEqual(transcricao(voz({ state: 'done', text: ' Me liga depois ', provider: 'openai' })), { texto: 'Me liga depois', nota: 'transcrito', provedor: 'OpenAI', erro: false })
  assert.deepEqual(transcricao(voz({ state: 'done', text: 'oi', provider: 'gemini' }))?.provedor, 'Gemini')
  assert.equal(transcricao(voz({ state: 'pending', text: null, provider: null }))?.nota, 'transcrevendo')
  assert.equal(transcricao(voz({ state: 'too_large', text: null, provider: null }))?.nota, 'longa')
  assert.deepEqual(transcricao(voz({ state: 'no_key', text: null, provider: null })), { texto: null, nota: 'semChave', provedor: null, erro: true })
  assert.equal(transcricao(voz({ state: 'failed', text: null, provider: null }))?.erro, true)
  // Opção desligada quando chegou, Hub antigo ou outra mídia: só "Mensagem de voz".
  assert.equal(transcricao(voz(null)), null)
  assert.equal(transcricao({ media: 'audio' }), null)
  assert.equal(transcricao({ media: 'image', transcript: { state: 'done', text: 'x', provider: 'openai' } }), null)
})

test('R8: apagar conversa tira só aquela da lista; aviso do wacli fica; erro do Hub volta e não some nada', async () => {
  let falhar = false
  let wacli = 'done'
  const c = chamador({
    donnaWhatsappChats: () => ({ chats: [conv({ whatsappChatId: 'c' }), conv({ whatsappChatId: 'd' })], unread: 0 }),
    donnaWhatsappChatErase: (b) => { if (falhar) throw new Error('bot_not_found: bot não encontrado.'); return { botId: b.botId, erased: 1, unread: 0, wacli, ...(wacli === 'failed' ? { warning: 'A cópia do wacli ficou.' } : {}) } },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  await caixa.apagarConversa('b', 'c')
  assert.deepEqual(c.chamadas.at(-1), ['donnaWhatsappChatErase', { botId: 'b', whatsappChatId: 'c' }])
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
  assert.equal(caixa.wacliFicou('b'), false)
  wacli = 'failed'
  await caixa.apagarConversa('b', 'd')
  assert.equal(caixa.wacliFicou('b'), true)
  await caixa.lerConversas('b')
  falhar = true
  await assert.rejects(caixa.apagarConversa('b', 'c'), /bot_not_found/)
  assert.equal(caixa.lista('b')?.conversas?.length, 2)
})

test('R3: wacli failed mantém a conversa aberta e avisa sem dizer que saiu; sucesso ainda tira da lista', async () => {
  let wacli = 'failed'
  const c = chamador({
    donnaWhatsappChats: () => ({ chats: [conv({ whatsappChatId: 'c' }), conv({ whatsappChatId: 'd' })], unread: 0 }),
    donnaWhatsappMessages: () => ({ whatsappChatId: 'd', messages: [msg(1, 10)], next: null }),
    donnaWhatsappChatErase: (b) => ({
      botId: b.botId, erased: wacli === 'done' ? 1 : 0, unread: 0, wacli,
      ...(wacli === 'failed' ? { warning: 'A conversa saiu da Mexerica.' } : {}),
    }),
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  await caixa.lerMensagens('b', 'd')
  await caixa.apagarConversa('b', 'd')
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['c', 'd'])
  assert.equal(caixa.conversa('b', 'd')?.msgs[0]?.messageId, 'm1')
  assert.equal(caixa.wacliFicou('b'), true)
  const aviso = traduzir('pt-BR', 'whatsapp.conversa.wacliFicou')
  assert.equal(aviso, 'Não consegui confirmar que a conversa saiu; tente apagar de novo.')
  assert.doesNotMatch(aviso, /A conversa saiu da Mexerica/)
  assert.equal(traduzir('en', 'whatsapp.conversa.wacliFicou'), 'Couldn’t confirm the chat was removed; try deleting it again.')
  assert.doesNotMatch(traduzir('en', 'whatsapp.conversa.wacliFicou'), /was removed from Mexerica/)
  assert.equal(traduzir('es', 'whatsapp.conversa.wacliFicou'), 'No pude confirmar que el chat salió; intenta eliminarlo de nuevo.')
  assert.doesNotMatch(traduzir('es', 'whatsapp.conversa.wacliFicou'), /salió de Mexerica/)
  assert.equal(traduzir('fr', 'whatsapp.conversa.wacliFicou'), 'Je n’ai pas pu confirmer que la conversation est partie ; réessayez de la supprimer.')
  assert.doesNotMatch(traduzir('fr', 'whatsapp.conversa.wacliFicou'), /retirée de Mexerica/)
  assert.equal(traduzir('zh-CN', 'whatsapp.conversa.wacliFicou'), '未能确认会话已删除；请再试一次删除。')
  assert.doesNotMatch(traduzir('zh-CN', 'whatsapp.conversa.wacliFicou'), /已从 Mexerica/)
  assert.equal(traduzir('ar-AE', 'whatsapp.conversa.wacliFicou'), 'تعذّر تأكيد أن المحادثة خرجت؛ حاول الحذف مرة أخرى.')
  assert.doesNotMatch(traduzir('ar-AE', 'whatsapp.conversa.wacliFicou'), /حُذفت المحادثة من Mexerica/)
  for (const idioma of ['en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) assert.notEqual(traduzir(idioma, 'whatsapp.conversa.wacliFicou'), aviso)
  wacli = 'done'
  await caixa.apagarConversa('b', 'c')
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
  assert.equal(caixa.conversa('b', 'd')?.msgs[0]?.messageId, 'm1')
  assert.equal(caixa.wacliFicou('b'), false)
})

test('R8: prazo de guarda 7/30/90, padrão 30 em Hub antigo; confirmação diz que o celular não muda', () => {
  assert.deepEqual([...PRAZOS], [7, 30, 90])
  assert.equal(prazoDe({} as AjustesBot), 30)
  assert.equal(prazoDe({ retentionDays: 7 } as AjustesBot), 7)
  // Confirmação (chaves de idioma): diz que o celular e o WhatsApp da pessoa não mudam, e fala da Mexerica.
  assert.match(traduzir('pt-BR', 'whatsapp.conversa.apagarContato', { nome: 'Carlos' }), /Nada muda no seu celular nem no WhatsApp de Carlos/)
  assert.match(traduzir('pt-BR', 'whatsapp.conversa.apagarGrupo', { nome: 'Clientes VIP' }), /do grupo Clientes VIP.*de quem está no grupo/)
  assert.match(traduzir('pt-BR', 'whatsapp.conversa.apagarContato', { nome: 'Carlos' }), /pela Mexerica/)
  assert.match(traduzir('en', 'whatsapp.conversa.apagarContato', { nome: 'Carlos' }), /Nothing changes on your phone or in Carlos’s WhatsApp/)
  assert.deepEqual(PRAZOS.map((d) => traduzirPlural('pt-BR', 'whatsapp.prazo.dias', d)), ['7 dias', '30 dias', '90 dias'])
  assert.deepEqual(PRAZOS.map((d) => traduzirPlural('en', 'whatsapp.prazo.dias', d)), ['7 days', '30 days', '90 days'])
})

test('foco ao abrir e voltar de uma conversa: o "‹" no painel, o título na tela grande; voltar foca a linha', () => {
  assert.equal(ALVO_AO_ABRIR.painel, '.wa-cvh .back')
  assert.equal(ALVO_AO_ABRIR.grande, '.wa-cvh .wa-cvh-t')
  // D387-SOL-01: a linha leva uma referência opaca, estável na sessão; nunca o JID (que tem o telefone).
  const a = seletorDaLinha('5511900000102@s.whatsapp.net')
  assert.match(a, /^\.wa-cv\[data-chat="c\d+"\]$/)
  assert.equal(seletorDaLinha('5511900000102@s.whatsapp.net'), a, 'estável')
  assert.notEqual(seletorDaLinha('120363000000002@g.us'), a, 'uma por conversa')
  assert.ok(!seletorDaLinha('a"b').includes('a"b'))
})

test('R8 (revisão W2): o que venceu ou passou do prazo novo sai da conversa aberta, também das páginas antigas já carregadas', async () => {
  const m = (id: string, receivedAt: number, sentAt: number): MensagemWa => ({ seq: sentAt, messageId: id, from: { name: 'Carlos', phone: null, contact: true }, call: null, sentAt, body: id, media: null, offline: false, read: true, receivedAt })
  let fase = 0
  const c = chamador({
    donnaWhatsappMessages: (b) => {
      if (b.before) return { whatsappChatId: 'c', messages: [m('MUITO-VELHA', 50, 50)], next: null, keepAfter: 10 }
      if (fase === 0) return { whatsappChatId: 'c', messages: [m('NEW', 1000, 1000), m('OLD', 100, 100)], next: { sentAt: 100, seq: 100 }, keepAfter: 10 }
      // Prazo encurtado no Hub: OLD e MUITO-VELHA ficaram para trás do corte.
      return { whatsappChatId: 'c', messages: [m('NEW', 1000, 1000)], next: null, keepAfter: 500 }
    },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerMensagens('b', 'c')
  await caixa.lerMensagens('b', 'c', true)
  assert.deepEqual(caixa.conversa('b', 'c')?.msgs.map((x) => x.messageId), ['NEW', 'OLD', 'MUITO-VELHA'])
  fase = 1
  await caixa.lerMensagens('b', 'c')
  assert.deepEqual(caixa.conversa('b', 'c')?.msgs.map((x) => x.messageId), ['NEW'])
})

test('R8 (revisão W3): uma leitura da lista que saiu antes de apagar não traz a conversa apagada de volta', async () => {
  let soltar: (v: unknown) => void = () => {}
  let segurar = false
  const c = chamador({
    donnaWhatsappChats: () => segurar ? new Promise((r) => { soltar = r }) : { chats: [conv({ whatsappChatId: 'c' }), conv({ whatsappChatId: 'd' })], unread: 0 },
    donnaWhatsappChatErase: (b) => ({ botId: b.botId, erased: 1, unread: 0, wacli: 'done' }),
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  segurar = true
  const atrasada = caixa.lerConversas('b')
  await caixa.apagarConversa('b', 'c')
  soltar({ chats: [conv({ whatsappChatId: 'c' }), conv({ whatsappChatId: 'd' })], unread: 0 })
  await atrasada
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
})

test('R8 (revisão W2, rodada 2): 30 → 7 → 90 com a releitura de 7 atrasada: texto e chamada apagados no Hub saem da tela', async () => {
  const agora = 100 * 86_400_000
  const dia = 86_400_000
  const m = (id: string, diasAtras: number, call = false): MensagemWa => ({ seq: agora - diasAtras * dia, messageId: id, from: { name: 'Carlos', phone: null, contact: true }, call: call ? { media: 'audio', durationSecs: 30 } : null, sentAt: agora - diasAtras * dia, body: id, media: null, offline: false, read: true, receivedAt: agora - diasAtras * dia })
  const NEW = m('NEW', 0)
  let soltar7: (v: unknown) => void = () => {}
  let fase = 30
  const c = chamador({
    donnaWhatsappMessages: () => {
      if (fase === 30) return { whatsappChatId: 'c', messages: [NEW, m('call:C1', 10, true), m('OLD', 10)], next: null, keepAfter: agora - 30 * dia, retentionRev: 0 }
      if (fase === 7) return new Promise((r) => { soltar7 = r })
      // 90 dias: o Hub já apagou OLD e a chamada na troca para 7; o corte de 90 não os alcança.
      return { whatsappChatId: 'c', messages: [NEW], next: null, keepAfter: agora - 90 * dia, retentionRev: 2 }
    },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerMensagens('b', 'c')
  assert.deepEqual(caixa.conversa('b', 'c')?.msgs.map((x) => x.messageId), ['NEW', 'call:C1', 'OLD'])
  fase = 7
  const atrasada = caixa.lerMensagens('b', 'c')
  fase = 90
  await caixa.lerMensagens('b', 'c')
  soltar7({ whatsappChatId: 'c', messages: [NEW], next: null, keepAfter: agora - 7 * dia, retentionRev: 1 })
  await atrasada
  assert.deepEqual(caixa.conversa('b', 'c')?.msgs.map((x) => x.messageId), ['NEW'])
})

test('R8 (revisão W2, rodada 2): com o prazo trocado, uma página antiga pedida depois recomeça da primeira', async () => {
  let rev = 0
  const m = (id: string, t: number): MensagemWa => ({ seq: t, messageId: id, from: { name: 'Carlos', phone: null, contact: true }, call: null, sentAt: t, body: id, media: null, offline: false, read: true, receivedAt: t })
  const c = chamador({
    donnaWhatsappMessages: (b) => b.before
      ? { whatsappChatId: 'c', messages: [m('VELHA', 50)], next: null, keepAfter: 0, retentionRev: rev }
      : rev === 0 ? { whatsappChatId: 'c', messages: [m('NEW', 1000), m('OLD', 100)], next: { sentAt: 100, seq: 100 }, keepAfter: 0, retentionRev: 0 }
      : { whatsappChatId: 'c', messages: [m('NEW', 1000)], next: null, keepAfter: 0, retentionRev: rev },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerMensagens('b', 'c')
  rev = 1
  await caixa.lerMensagens('b', 'c', true)
  await new Promise((r) => setTimeout(r, 0))
  assert.deepEqual(caixa.conversa('b', 'c')?.msgs.map((x) => x.messageId), ['NEW'])
})

test('R3: aviso do Hub durante apagar failed mantém a conversa para repetir e sucesso a remove', async () => {
  let vazio = false
  let falhar = true
  let concluir!: (value: { wacli: string }) => void
  const resposta = new Promise<{ wacli: string }>((resolve) => { concluir = resolve })
  const c = chamador({
    donnaWhatsappChats: () => ({ chats: vazio ? [] : [conv({ whatsappChatId: 'd' })], unread: 0 }),
    donnaWhatsappMessages: () => ({ whatsappChatId: 'd', messages: vazio ? [] : [msg(1, 10)], next: null }),
    donnaWhatsappChatErase: () => {
      vazio = true
      return falhar ? resposta : { wacli: 'done' }
    },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  await caixa.lerMensagens('b', 'd')
  const apagar = caixa.apagarConversa('b', 'd')
  // O Hub avisa logo depois do primeiro DELETE, antes de saber se o wacli terminou.
  caixa.aviso('b')
  await new Promise<void>((resolve) => { setImmediate(resolve) })
  assert.ok(c.chamadas.filter(([method]) => method === 'donnaWhatsappChats').length >= 2)
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
  concluir({ wacli: 'failed' })
  await apagar
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
  assert.equal(caixa.conversa('b', 'd')?.msgs[0]?.messageId, 'm1')
  assert.equal(caixa.wacliFicou('b'), true)
  // Um aviso entregue depois da resposta também não tira a tentativa que falhou.
  caixa.aviso('b')
  await new Promise<void>((resolve) => { setImmediate(resolve) })
  assert.deepEqual(caixa.lista('b')?.conversas?.map((x) => x.whatsappChatId), ['d'])
  falhar = false
  await caixa.apagarConversa('b', 'd')
  caixa.aviso('b')
  await new Promise<void>((resolve) => { setImmediate(resolve) })
  assert.deepEqual(caixa.lista('b')?.conversas, [])
  assert.equal(caixa.conversa('b', 'd'), null)
  assert.equal(caixa.wacliFicou('b'), false)
})

// MX-18: a marca "responde sozinho" e a escrita direta do dono no mini WhatsApp.

test('MX-18: marcar conversa atualiza a lista na hora; erro do Hub volta cru', async () => {
  let falhar = false
  const c = chamador({
    donnaWhatsappChats: () => ({ chats: [conv({ whatsappChatId: 'c' }), conv({ whatsappChatId: 'd', marked: true })], unread: 0 }),
    donnaWhatsappChatMark: (b) => { if (falhar) throw new Error('not_allowed: só o dono pode mudar o WhatsApp.'); return { botId: b.botId, whatsappChatId: b.whatsappChatId, marked: b.marked } },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  assert.equal(caixa.lista('b')?.conversas?.[1]?.marked, true)
  await caixa.marcarConversa('b', 'c', true)
  assert.deepEqual(c.chamadas.at(-1), ['donnaWhatsappChatMark', { botId: 'b', whatsappChatId: 'c', marked: true }])
  assert.equal(caixa.lista('b')?.conversas?.[0]?.marked, true)
  await caixa.marcarConversa('b', 'd', false)
  assert.equal(caixa.lista('b')?.conversas?.[1]?.marked, false)
  falhar = true
  await assert.rejects(caixa.marcarConversa('b', 'c', false), /só o dono/)
  // A lista não muda quando o Hub recusa.
  assert.equal(caixa.lista('b')?.conversas?.[0]?.marked, true)
})

test('MX-105: autônomo do grupo muda só após o Hub aceitar', async () => {
  let falhar = false
  const c = chamador({
    donnaWhatsappChats: () => ({ chats: [conv({ whatsappChatId: 'g', kind: 'group', marked: true, groupAuto: false })], unread: 0 }),
    donnaWhatsappGroupAuto: () => { if (falhar) throw new Error('not_allowed'); return {} },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  await caixa.lerConversas('b')
  await caixa.alterarAutonomoGrupo('b', 'g', true)
  assert.deepEqual(c.chamadas.at(-1), ['donnaWhatsappGroupAuto', { botId: 'b', whatsappChatId: 'g', auto: true }])
  assert.equal(caixa.lista('b')?.conversas?.[0]?.groupAuto, true)
  falhar = true
  await assert.rejects(caixa.alterarAutonomoGrupo('b', 'g', false), /not_allowed/)
  assert.equal(caixa.lista('b')?.conversas?.[0]?.groupAuto, true)
})

test('MX-18: o dono escreve direto — sai ou fica incerto; sem cliente é SEM_CONEXAO', async () => {
  let incerto = false
  const c = chamador({
    donnaWhatsappSend: () => incerto ? { state: 'uncertain' } : { state: 'sent', id: 'w1' },
    donnaWhatsapp: () => ({ plugin: { enabled: true, mode: 'per_bot' }, bots: [] }),
  })
  const caixa = new CaixaWhatsapp(() => c)
  assert.equal(await caixa.enviarDono('b', 'c', 'oi'), 'sent')
  assert.deepEqual(c.chamadas.at(-1), ['donnaWhatsappSend', { botId: 'b', whatsappChatId: 'c', text: 'oi' }])
  incerto = true
  assert.equal(await caixa.enviarDono('b', 'c', 'oi de novo'), 'uncertain')
  const sem = new CaixaWhatsapp(() => null)
  await assert.rejects(sem.enviarDono('b', 'c', 'oi'), new Error('sem_conexao'))
})
