// whatsapp R5, só na demo: os ajustes do WhatsApp em memória, com as regras do Hub (host/src/fork/whatsapp,
// R1 e número do R2). Nenhum número real, QR de verdade ou rede: o "QR" é só o texto EXEMPLO e o código é
// fictício. Serve para ver as telas com dados de exemplo.

type Ajustes = { readChats: boolean; replyChats: boolean; readGroups: boolean; replyGroups: boolean; replyMode: 'approval' | 'autonomous'; delaySeconds: number; hourlyLimit: number; dailyLimit?: number; quietEnabled?: boolean; quietStart?: number; quietEnd?: number; transcribeAudio: boolean; retentionDays: number }
const padrao = (): Ajustes => ({ readChats: false, replyChats: false, readGroups: false, replyGroups: false, replyMode: 'approval', delaySeconds: 15, hourlyLimit: 30, dailyLimit: 100, quietEnabled: true, quietStart: 1320, quietEnd: 480, transcribeAudio: true, retentionDays: 30 })
const ACESSOS = ['readChats', 'replyChats', 'readGroups', 'replyGroups'] as const
type Resultado = 'waiting' | 'connected' | 'expired' | 'refused' | 'cancelled'
/** Gravado: 'connected' | 'disconnected' | 'needs_reconnect'; o resto sai na leitura. */
type Numero = { gravado?: 'connected' | 'disconnected' | 'needs_reconnect'; pairing?: { method: 'code' | 'qr'; result: Resultado; segredo?: string } }
const METODOS = ['donnaWhatsapp', 'donnaWhatsappSave', 'donnaWhatsappBotSave', 'donnaWhatsappPair', 'donnaWhatsappPairCancel', 'donnaWhatsappDisconnect', 'donnaWhatsappWacliRetry',
  // MX-18: a fila, a marca e a escrita do dono respondem mesmo sem a caixa de exemplo.
  'donnaWhatsappQueue', 'donnaWhatsappQueueDecide', 'donnaWhatsappChatMark', 'donnaWhatsappGroupAuto', 'donnaWhatsappSend', 'donnaWhatsappSendDraft']

export class FakeWhatsapp {
  enabled = false
  mode: 'donna_only' | 'per_bot' = 'donna_only'
  /** Só para prints: força a falha de leitura. */
  falhar = false
  /** Só para prints: wacli ausente e falha ao avisar o WhatsApp no Desconectar. */
  /** R7: o download do wacli; `instalado` só com `ready`. */
  wacli: { state: 'ready' | 'downloading' | 'failed' | 'unsupported'; reason?: string } = { state: 'ready' }
  get instalado() { return this.wacli.state === 'ready' }
  set instalado(v: boolean) { this.wacli = v ? { state: 'ready' } : { state: 'failed', reason: 'wacli_download: não foi possível baixar do GitHub.' } }
  logoutFalha = false
  private bots = new Map<string, Ajustes>()
  private numeros = new Map<string, Numero>()
  private qrs = 0
  /** MX-18, só na demo: a fila de saída de exemplo do bot da caixa. */
  private readonly fila: { id: string; botId: string; chatId: string; isGroup: boolean; replyTo: string; text: string; origin: string; state: string; reason: string | null; notBefore: number; attempts: number; sentAt: number | null; createdAt: number }[] = []

  /** R5c, só com `?whatsapp=<cena>`: um bot com número e a caixa de exemplo (caixaDemo abaixo). */
  private readonly caixa = caixaDemo()

  constructor(private readonly agentes: () => string[], private readonly donna: () => string | null, private readonly avisar: (botId: string) => void = () => {}) {
    if (!this.caixa) return
    this.enabled = true
    this.mode = 'per_bot'
    const ver = this.caixa.cena !== 'ver-desligado'
    this.bots.set(CAIXA_BOT, { ...padrao(), readChats: ver, readGroups: ver, replyChats: ver, replyGroups: ver })
    this.numeros.set(CAIXA_BOT, { gravado: this.caixa.cena === 'desconectado' ? 'disconnected' : 'connected' })
    const agora = Date.now()
    this.fila.push(
      { id: 'demo-fila-1', botId: CAIXA_BOT, chatId: '5511900000102@s.whatsapp.net', isGroup: false, replyTo: 'm1', text: 'Oi Carlos! O orçamento sai ainda hoje, te mando até as 18h.', origin: 'owner_card', state: 'sent', reason: null, notBefore: 0, attempts: 1, sentAt: agora - 42 * 60_000, createdAt: agora - 3_600_000 },
      { id: 'demo-fila-2', botId: CAIXA_BOT, chatId: '5511900000103@s.whatsapp.net', isGroup: false, replyTo: 'm2', text: 'Oi Beatriz! Posso te ligar depois do almoço, que horas fica bom?', origin: 'bot_routine', state: 'held', reason: 'quiet', notBefore: agora + 3_600_000, attempts: 0, sentAt: null, createdAt: agora - 1_800_000 },
      { id: 'demo-fila-3', botId: CAIXA_BOT, chatId: '5511900000107@s.whatsapp.net', isGroup: false, replyTo: 'm3', text: 'Olá! Vi seu contato pelo anúncio. Posso te ajudar?', origin: 'bot_routine', state: 'uncertain', reason: 'uncertain', notBefore: 0, attempts: 1, sentAt: null, createdAt: agora - 900_000 },
    )
  }

  /** MX-18: a fila do bot, como `donnaWhatsappQueue` do Hub. */
  private filaDo(botId: string) {
    return { items: this.fila.filter((i) => i.botId === botId) }
  }

  atende = (method: string) => METODOS.includes(method) || (!!this.caixa && method.startsWith('donnaWhatsapp'))

  /** Controles dos prints: o estado do número, o fim do pareamento e a troca do QR. */
  numero(botId: string, gravado: Numero['gravado']) { this.numeros.set(botId, { gravado }); this.avisar(botId) }
  concluir(botId: string, result: Exclude<Resultado, 'waiting'>) {
    const n = this.numeros.get(botId)
    if (!n?.pairing) return
    n.pairing = { method: n.pairing.method, result }
    if (result === 'connected') n.gravado = 'connected'
    this.avisar(botId)
  }
  trocarQr(botId: string) {
    const p = this.numeros.get(botId)?.pairing
    if (p?.method === 'qr' && p.result === 'waiting') { p.segredo = `EXEMPLO-DEMO-NAO-CONECTA-${++this.qrs}`; this.avisar(botId) }
  }

  call(method: string, b: Record<string, unknown>, fail: (message: string) => unknown): unknown {
    if (this.falhar) throw fail('O computador demorou para responder. Tente de novo.')
    const c = this.caixa
    if (c && method === 'donnaWhatsappChats') {
      if (c.cena === 'erro') throw fail('O computador demorou para responder. Tente de novo.')
      return { botId: b.botId, chats: b.botId === CAIXA_BOT && c.cena !== 'vazio' ? c.chats : [], unread: c.unread() }
    }
    if (c && method === 'donnaWhatsappMessages') {
      const chat = c.chats.find((x) => x.whatsappChatId === b.whatsappChatId)
      return { botId: b.botId, whatsappChatId: b.whatsappChatId, kind: chat?.kind ?? 'individual', name: chat?.name ?? null, messages: c.mensagens(String(b.whatsappChatId)), next: null }
    }
    if (c && method === 'donnaWhatsappMarkRead') {
      const chat = c.chats.find((x) => x.whatsappChatId === b.whatsappChatId)
      if (chat?.unread) { chat.unread = 0; setTimeout(() => this.avisar(String(b.botId)), 0) }
      return { botId: b.botId, whatsappChatId: b.whatsappChatId, unread: c.unread() }
    }
    if (c && method === 'donnaWhatsappChatErase') {
      // R8: só a caixa deste computador; o demo não tem wacli.
      const antes = c.chats.length
      c.chats.splice(0, c.chats.length, ...c.chats.filter((x) => x.whatsappChatId !== b.whatsappChatId))
      setTimeout(() => this.avisar(String(b.botId)), 0)
      return { botId: b.botId, whatsappChatId: b.whatsappChatId, erased: antes - c.chats.length, unread: c.unread(), wacli: 'skipped' }
    }
    // MX-18: a marca "responde sozinho" e a escrita direta do dono.
    if (method === 'donnaWhatsappChatMark') {
      const chat = c?.chats.find((x) => x.whatsappChatId === b.whatsappChatId)
      if (chat) {
        const estado = chat as { marked?: boolean; groupAuto?: boolean }
        estado.marked = b.marked === true
        if (!estado.marked) estado.groupAuto = false
      }
      return { botId: b.botId, whatsappChatId: b.whatsappChatId, marked: b.marked === true }
    }
    if (method === 'donnaWhatsappGroupAuto') {
      const chat = c?.chats.find((x) => x.whatsappChatId === b.whatsappChatId)
      const estado = chat as { kind?: string; marked?: boolean; groupAuto?: boolean } | undefined
      if (!estado || estado.kind !== 'group' || !estado.marked) throw fail('Marque o grupo antes de mudar o envio autônomo.')
      estado.groupAuto = b.auto === true
      return { botId: b.botId, whatsappChatId: b.whatsappChatId, auto: estado.groupAuto }
    }
    if (method === 'donnaWhatsappSend') return { state: 'sent', id: 'demo-envio' }
    if (method === 'donnaWhatsappSendDraft') return { state: 'queued' }
    if (method === 'donnaWhatsappQueue') return this.filaDo(String(b.botId))
    if (method === 'donnaWhatsappQueueDecide') {
      const item = this.fila.find((i) => i.id === b.itemId && i.botId === b.botId)
      if (item && (b.decision === 'sent' || b.decision === 'cancelled')) {
        item.state = b.decision === 'sent' ? 'sent' : 'cancelled'
        if (b.decision === 'sent') item.sentAt = Date.now()
      }
      return this.filaDo(String(b.botId))
    }
    if (method === 'donnaWhatsappSave') {
      if (b.mode === 'donna_only' || b.mode === 'per_bot') this.mode = b.mode
      if (typeof b.enabled === 'boolean' && b.enabled !== this.enabled) {
        this.enabled = b.enabled
        if (!b.enabled) for (const s of this.bots.values()) for (const k of ACESSOS) s[k] = false
      }
    } else if (method === 'donnaWhatsappBotSave') {
      const id = String(b.botId)
      const p = (b.settings ?? {}) as Partial<Ajustes>
      if (!this.enabled) throw fail('plugin_disabled: ligue o plugin WhatsApp primeiro.')
      if (!this.agentes().includes(id)) throw fail('bot_not_found: bot não encontrado.')
      const s = { ...(this.bots.get(id) ?? padrao()) }
      if (p.readChats === false && p.replyChats === undefined) s.replyChats = false
      if (p.readGroups === false && p.replyGroups === undefined) s.replyGroups = false
      Object.assign(s, p)
      if (s.replyChats && !s.readChats) throw fail('invalid_field: responder conversas exige ver conversas.')
      if (s.replyGroups && !s.readGroups) throw fail('invalid_field: responder grupos exige ver grupos.')
      if (!(s.delaySeconds >= 8 && s.delaySeconds <= 300)) throw fail('invalid_field: o intervalo entre mensagens vai de 8 a 300 segundos. Nunca mais rápido que 1 mensagem a cada 8 s.')
      if (!(s.hourlyLimit >= 1 && s.hourlyLimit <= 200)) throw fail('invalid_field: o limite vai de 1 a 200 mensagens por hora. Você pode baixar, nunca passar de 200.')
      if (s.dailyLimit !== undefined && !(s.dailyLimit >= 1 && s.dailyLimit <= 500)) throw fail('invalid_field: o limite por dia vai de 1 a 500 mensagens.')
      if (s.replyMode !== undefined && s.replyMode !== 'approval' && s.replyMode !== 'autonomous') throw fail('invalid_field: modo de resposta inválido.')
      if ((s.quietStart !== undefined && !(s.quietStart >= 0 && s.quietStart <= 1439)) || (s.quietEnd !== undefined && !(s.quietEnd >= 0 && s.quietEnd <= 1439))) throw fail('invalid_field: horário de silêncio inválido.')
      if (![7, 30, 90].includes(s.retentionDays)) throw fail('invalid_field: o prazo é 7, 30 ou 90 dias.')
      if (this.mode === 'donna_only' && id !== this.donna() && ACESSOS.some((k) => s[k])) throw fail('not_allowed: no modo Só o Berga, só o Berga usa o WhatsApp.')
      this.bots.set(id, s)
    } else if (method === 'donnaWhatsappPair') {
      const id = String(b.botId)
      if (!this.enabled) throw fail('plugin_disabled: ligue o plugin WhatsApp primeiro.')
      if (this.mode === 'donna_only' && id !== this.donna()) throw fail('not_allowed: no modo Só o Berga, só o Berga usa o WhatsApp.')
      const n = this.numeros.get(id) ?? {}
      if (n.gravado === 'connected' || n.gravado === 'disconnected') throw fail('already_connected: desconecte o número primeiro.')
      if (!this.instalado) throw fail('wacli_not_installed: o wacli não está instalado neste computador.')
      const code = b.method === 'code'
      if (code && String(b.phone ?? '').replace(/\D/g, '').length < 10) throw fail('invalid_field: número de telefone inválido.')
      n.pairing = { method: code ? 'code' : 'qr', result: 'waiting', segredo: code ? 'EXEMPLO1' : `EXEMPLO-DEMO-NAO-CONECTA-${++this.qrs}` }
      this.numeros.set(id, n)
      this.avisar(id)
    } else if (method === 'donnaWhatsappWacliRetry') {
      if (this.wacli.state === 'failed') this.wacli = { state: 'downloading' }
    } else if (method === 'donnaWhatsappPairCancel') {
      this.concluir(String(b.botId), 'cancelled')
    } else if (method === 'donnaWhatsappDisconnect') {
      const id = String(b.botId)
      const antes = this.numeros.get(id)?.gravado
      this.numeros.delete(id)
      this.avisar(id)
      const logout = !antes || antes === 'needs_reconnect' ? 'skipped' : this.logoutFalha ? 'failed' : 'done'
      return { ...this.ler(), disconnect: logout === 'failed' ? { logout, warning: 'Não deu para avisar o WhatsApp. No celular do número, abra Aparelhos conectados e remova este aparelho.' } : { logout } }
    }
    return this.ler()
  }

  private ler() {
    const donna = this.donna()
    return {
      plugin: { enabled: this.enabled, mode: this.mode, wacli: this.enabled ? { installed: this.instalado, version: 'v0.25.1', ...this.wacli } : { installed: this.instalado, state: 'off', version: 'v0.25.1' } },
      transcription: { provider: 'openai', model: 'gpt-4o-mini-transcribe', source: 'default', keyOnHub: true },
      bots: this.agentes().map((id) => {
        const settings = this.bots.get(id) ?? padrao()
        const isDonna = id === donna
        const paused = this.mode === 'donna_only' && !isDonna
        const live = this.enabled && !paused
        const caixa = this.caixa && id === CAIXA_BOT
        return {
          botId: id, isDonna, paused, settings, number: this.numeroDe(id, live), sending: { state: 'none' }, ...(caixa ? { inbox: { unread: this.caixa!.unread() } } : {}),
          botCan: Object.fromEntries(ACESSOS.map((k) => [k, live && settings[k]])),
        }
      }),
    }
  }

  /** Como `number.rs::view` do R2. */
  private numeroDe(id: string, live: boolean) {
    const n = this.numeros.get(id)
    const p = n?.pairing
    const ativo = p?.result === 'waiting'
    const state = n?.gravado === 'connected' || n?.gravado === 'disconnected'
      ? (!this.instalado ? 'not_installed' : !live ? 'stopped' : n.gravado)
      : n?.gravado === 'needs_reconnect' && !ativo ? 'needs_reconnect'
      : ativo ? 'pairing' : !this.instalado ? 'not_installed' : 'none'
    const pairing = p ? { method: p.method, result: p.result, ...(ativo && p.segredo ? { [p.method]: p.segredo } : {}) } : undefined
    // R5c: o telefone só com o número conectado (como o Hub), e só o fictício da caixa do demo.
    const phone = state === 'connected' && this.caixa && id === CAIXA_BOT ? { phone: '+5511900000101' } : {}
    return { state, ...phone, ...(pairing ? { pairing } : {}) }
  }
}

// R5c, só na demo: a caixa de exemplo da proposta aprovada (docs/reference/redesenho/propostas/whatsapp-mini-20261106),
// com dados fictícios. `?whatsapp=lista|vazio|desconectado|ver-desligado|erro`. Nenhum número real, nada de rede.
const CAIXA_BOT = 'prosa'
type Msg = { seq: number; messageId: string; from: { name: string; phone: string | null; contact: boolean }; call: { media: string | null; durationSecs: number | null } | null; sentAt: number; body: string; media: string | null; offline: boolean; read: boolean; transcript?: Transcricao | null }
type Transcricao = { state: string; text: string | null; provider: string | null }

function caixaDemo() {
  const cena = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('whatsapp') : null
  if (cena === null) return null
  const hoje = new Date()
  const at = (diasAtras: number, h: number, m: number) => new Date(hoje.getFullYear(), hoje.getMonth(), hoje.getDate() - diasAtras, h, m).getTime()
  const last = (senderName: string, body: string, sentAt: number, o: { contact?: boolean; media?: string; call?: string } = {}) =>
    ({ senderName, senderContact: o.contact ?? true, body, media: o.media ?? null, call: o.call ? { media: o.call, durationSecs: null } : null, sentAt, offline: false })
  const chats = [
    { whatsappChatId: '120363000000001@g.us', kind: 'group', name: 'Fornecedores (exemplo)', phone: null, pinned: true, unread: 0, last: last('+5511900000301', 'Entrega confirmada pra sexta.', at(3, 15, 10), { contact: false }) },
    { whatsappChatId: '5511900000102@s.whatsapp.net', kind: 'individual', name: 'Carlos (exemplo)', phone: '+5511900000102', pinned: false, unread: 2, last: last('Carlos (exemplo)', 'Pode mandar o orçamento? Se der, ainda hoje', at(0, 10, 42)) },
    { whatsappChatId: '120363000000002@g.us', kind: 'group', name: 'Clientes VIP (exemplo)', phone: null, pinned: false, unread: 1, last: last('Marina', 'Fechado, obrigada Prosa!', at(0, 10, 31), { contact: false }) },
    { whatsappChatId: '5511900000107@s.whatsapp.net', kind: 'individual', name: null, phone: '+5511900000107', pinned: false, unread: 1, last: last('+5511900000107', 'Oi, vi o anúncio de vocês', at(0, 9, 58)) },
    { whatsappChatId: '5511900000103@s.whatsapp.net', kind: 'individual', name: 'Beatriz (exemplo)', phone: '+5511900000103', pinned: false, unread: 0, marked: true, last: last('Beatriz (exemplo)', 'Mensagem de voz', at(0, 9, 20), { media: 'audio' }) },
    { whatsappChatId: '5511900000104@s.whatsapp.net', kind: 'individual', name: 'Rodrigo (exemplo)', phone: '+5511900000104', pinned: false, unread: 0, last: last('Rodrigo (exemplo)', 'Foto', at(1, 18, 5), { media: 'image' }) },
    { whatsappChatId: '5511900000105@s.whatsapp.net', kind: 'individual', name: 'Juliana (exemplo)', phone: '+5511900000105', pinned: false, unread: 0, last: last('Juliana (exemplo)', 'Chamada de voz', at(4, 11, 0), { call: 'audio' }) },
    { whatsappChatId: '5511900000106@s.whatsapp.net', kind: 'individual', name: 'Mateus (exemplo)', phone: '+5511900000106', pinned: false, unread: 0, last: last('Mateus (exemplo)', 'Obrigado pelo retorno!', at(9, 16, 40)) },
  ]
  let seq = 0
  const msg = (name: string, body: string, sentAt: number, o: { contact?: boolean; media?: string; offline?: boolean; phone?: string; transcript?: Transcricao } = {}): Msg =>
    ({ seq: ++seq, messageId: `demo-${seq}`, from: { name, phone: o.phone ?? null, contact: o.contact ?? true }, call: null, sentAt, body, media: o.media ?? null, offline: o.offline ?? false, read: true, transcript: o.transcript ?? null })
  // R3b: a mensagem de voz transcrita e os estados da transcrição (dados fictícios).
  const voz = (state: string, text: string | null = null, provider: string | null = null): Transcricao => ({ state, text, provider })
  const conversas: Record<string, Msg[]> = {
    '5511900000102@s.whatsapp.net': [
      msg('Carlos (exemplo)', 'Boa tarde! Vocês fazem entrega em Campinas?', at(1, 17, 2)),
      msg('Carlos (exemplo)', 'Mensagem de voz', at(0, 7, 31), { media: 'audio', offline: true, transcript: voz('done', 'Bom dia! Queria saber se vocês entregam em Campinas ainda esta semana. Se der, me manda o valor do frete.', 'openai') }),
      msg('Carlos (exemplo)', 'Foto', at(0, 7, 33), { media: 'image', offline: true }),
      msg('Carlos (exemplo)', 'Pode mandar o orçamento? Se der, ainda hoje', at(0, 10, 42)),
    ],
    '5511900000103@s.whatsapp.net': [
      msg('Beatriz (exemplo)', 'Mensagem de voz', at(0, 8, 2), { media: 'audio', transcript: voz('done', 'Oi! Consegue me ligar depois do almoço?', 'gemini') }),
      msg('Beatriz (exemplo)', 'Mensagem de voz', at(0, 8, 40), { media: 'audio', transcript: voz('too_large') }),
      msg('Beatriz (exemplo)', 'Mensagem de voz', at(0, 9, 1), { media: 'audio', transcript: voz('failed') }),
      msg('Beatriz (exemplo)', 'Mensagem de voz', at(0, 9, 12), { media: 'audio', transcript: voz('no_key') }),
      msg('Beatriz (exemplo)', 'Mensagem de voz', at(0, 9, 20), { media: 'audio', transcript: voz('pending') }),
    ],
    '120363000000002@g.us': [
      msg('Marina', 'Vale pra compra pelo site também?', at(0, 9, 12), { contact: false, phone: '+5511900000201' }),
      msg('+5511900000202', 'Mensagem de voz', at(0, 9, 40), { contact: false, media: 'audio', phone: '+5511900000202', transcript: voz('pending') }),
      msg('Marina', 'Fechado, obrigada Prosa!', at(0, 10, 31), { contact: false, phone: '+5511900000201' }),
    ],
  }
  return {
    cena,
    chats,
    unread: () => chats.reduce((n, c) => n + c.unread, 0),
    mensagens: (id: string) => {
      const c = chats.find((x) => x.whatsappChatId === id)
      if (!conversas[id] && c) conversas[id] = [{ ...msg(c.last.senderName, c.last.body, c.last.sentAt, { media: c.last.media ?? undefined }), call: c.last.call }]
      return [...(conversas[id] ?? [])].reverse()
    },
  }
}
