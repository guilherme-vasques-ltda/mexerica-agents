import assert from 'node:assert/strict'
import test from 'node:test'
import { FIXED_ACTIVITY_KINDS, type ActivityCode } from '../../shared/fork-activity-code.ts'
import { APP_LANGUAGES, type AppLanguage } from '../../shared/fork-idioma.ts'
import { normalizeBot } from '../../shared/models.ts'
import { waitingLabel } from '../../shared/subscription.ts'
import { activityCodeText, activityOriginal, activityText } from './fork-atividade.ts'
import { CATALOGOS, tradutor, traduzir } from './fork-i18n/index.ts'
import { stepText } from '../views/thread/cards/step-text.ts'
import { activityIn } from '../views/fork/donna/DonnabotThinkingMark.ts'

// MX-147: o código vira texto do catálogo no idioma de `t`; sem código válido (ou sem como reproduzir), `activity`
// passa intacto.

const FIXED_KEY = {
  writing_reply: 'navegacao.barra.atividade.escrevendo', thinking: 'navegacao.barra.atividade.pensando', working: 'navegacao.barra.atividade.trabalhando',
  starting_agent: 'navegacao.barra.atividade.iniciando', resuming_session: 'navegacao.barra.atividade.retomando', continuing: 'navegacao.barra.atividade.continuando',
  typing: 'navegacao.barra.atividade.escrevendo', starting: 'navegacao.barra.atividade.iniciando', summarizing: 'navegacao.barra.atividade.resumindo',
  resuming_work: 'navegacao.barra.atividade.retomandoTrabalho', waiting_admission: 'navegacao.barra.atividade.aguardandoAdmissao',
  waiting_session: 'navegacao.barra.atividade.aguardandoSessao', subscription_paused: 'navegacao.barra.atividade.assinaturaSuspensa',
  setup_failed: 'navegacao.barra.atividade.preparoFalhou', routine_setup_failed: 'navegacao.barra.atividade.rotinaPreparoFalhou',
  routine_interrupted: 'navegacao.barra.atividade.rotinaInterrompida', start_failed: 'navegacao.barra.atividade.naoIniciou',
} as const
const texto = (lang: AppLanguage, code: ActivityCode, resumeAt?: number | null) => activityCodeText(tradutor(lang), lang, code, resumeAt)!

test('os seis idiomas: cada kind fixo sai do catálogo, sem chave solta; só os pares starting/starting_agent e typing/writing_reply dividem texto', () => {
  assert.equal(APP_LANGUAGES.length, 6)
  const sem = ['subscription_wait']
  assert.deepEqual(Object.keys(FIXED_KEY).sort(), FIXED_ACTIVITY_KINDS.filter((k) => !sem.includes(k)).sort(), 'todo kind fixo tem texto')
  for (const lang of APP_LANGUAGES) {
    const textos = new Map<string, string>()
    for (const [kind, key] of Object.entries(FIXED_KEY)) {
      const out = texto(lang, { kind } as ActivityCode)
      assert.equal(out, CATALOGOS[lang][key], `${lang}/${kind}`)
      assert.ok(out && !out.includes('navegacao.barra') && !/\{\w+\}/.test(out), `${lang}/${kind}`)
      textos.set(kind, out)
    }
    assert.equal(new Set(textos.values()).size, textos.size - 2, `${lang}: só os dois pares coincidem`)
    // typing diz o mesmo que a frase legada "Typing…" (#712): sem variar entre Hub novo e antigo.
    assert.equal(texto(lang, { kind: 'typing' }), activityIn(lang, 'Typing…'), `${lang}: typing = legado`)
  }
  assert.equal(texto('pt-BR', { kind: 'typing' }), 'Escrevendo…')
  assert.equal(texto('en', { kind: 'typing' }), 'Writing…')
  assert.equal(texto('pt-BR', { kind: 'start_failed' }), 'Não foi possível iniciar')
})

test('parâmetros: title e item entram no texto de cada idioma, isolados da direita para a esquerda; tool passa pela linguagem simples (MX-253)', () => {
  for (const lang of APP_LANGUAGES) {
    const rtl = lang === 'ar-AE'
    const iso = (v: string) => (rtl ? `⁨${v}⁩` : v)
    assert.ok(texto(lang, { kind: 'needs_approval', title: 'Edit src/pace.js' }).includes(iso('Edit src/pace.js')), lang)
    assert.ok(texto(lang, { kind: 'downloading', item: 'extrato.pdf' }).includes(iso('extrato.pdf')), lang)
    assert.ok(texto(lang, { kind: 'installing', item: 'Node.js' }).includes(iso('Node.js')), lang)
    // MX-253: o título da ferramenta passa pela tradução em linguagem simples dos passos (o caminho fica isolado em árabe).
    assert.equal(texto(lang, { kind: 'tool', title: 'Read src/pace.js' }), stepText(tradutor(lang), { title: 'Read src/pace.js', kind: 'other' }), lang)
    assert.ok(texto(lang, { kind: 'tool', title: 'Read src/pace.js' }).includes(iso('src/pace.js')), lang)
  }
  assert.equal(texto('pt-BR', { kind: 'needs_approval', title: 'Editar a.ts' }), 'Pede aprovação: Editar a.ts')
  assert.equal(texto('en', { kind: 'downloading', item: 'a.pdf' }), 'Downloading a.pdf…')
  assert.equal(texto('pt-BR', { kind: 'installing', item: 'Node.js' }), 'Instalando Node.js…')
  // O parâmetro é texto do Hub: chaves dentro dele não são interpretadas.
  assert.equal(texto('en', { kind: 'downloading', item: '{item}' }), 'Downloading {item}…')
})

test('member: o prefixo "Nome: " do grupo é preservado em todos os idiomas e em todo tipo de código', () => {
  const codes: ActivityCode[] = [{ kind: 'thinking', member: 'Byte' }, { kind: 'tool', title: 'Edit a.ts', member: 'Byte' }, { kind: 'needs_approval', title: 'Edit a.ts', member: 'Byte' }]
  for (const lang of APP_LANGUAGES) {
    for (const code of codes) {
      const sem = texto(lang, { ...code, member: undefined } as ActivityCode)
      const com = texto(lang, code)
      assert.ok(com.includes(lang === 'ar-AE' ? '⁨Byte⁩' : 'Byte') && com.includes(lang === 'ar-AE' ? `⁨${sem}⁩` : sem), `${lang}/${code.kind}: ${com}`)
    }
  }
  assert.equal(texto('en', { kind: 'tool', title: 'Edit a.ts', member: 'Byte' }), 'Byte: Changing a.ts')
  assert.equal(texto('pt-BR', { kind: 'thinking', member: 'Byte' }), 'Byte: Pensando…')
})

test('subscription_wait: horário do resumeAt do bot, "assim que liberar" sem hora, legado quando não reproduzível', () => {
  const at = Date.UTC(2026, 10, 11, 15, 30)
  const t = tradutor('pt-BR')
  for (const lang of APP_LANGUAGES) {
    const t2 = tradutor(lang)
    assert.equal(texto(lang, { kind: 'subscription_wait' }, at), waitingLabel(t2, lang, { resumeAt: at }), lang)
    assert.equal(texto(lang, { kind: 'subscription_wait' }, null), waitingLabel(t2, lang, { resumeAt: null }), lang)
  }
  assert.equal(texto('pt-BR', { kind: 'subscription_wait' }, null), 'Aguardando · assim que liberar')
  assert.match(texto('pt-BR', { kind: 'subscription_wait' }, at), /^Aguardando · retomada às \d{2}:\d{2}$/)
  assert.equal(texto('pt-BR', { kind: 'subscription_wait', member: 'Byte' }, null), 'Byte: Aguardando · assim que liberar')
  // Horário inválido não se reproduz: volta ao texto do Hub.
  for (const resumeAt of [Number.NaN, Infinity, '15:30' as never, 8.7e15]) {
    assert.equal(activityText(t, 'pt-BR', { activity: 'Aguardando · retomada às 15:30', activityCode: { kind: 'subscription_wait' }, resumeAt }), 'Aguardando · retomada às 15:30', String(resumeAt))
  }
})

test('fallback: sem código, ou com código desconhecido/malformado, vale o activity do Hub, intacto', () => {
  const t = tradutor('pt-BR')
  assert.equal(activityText(t, 'pt-BR', { activity: 'Writing a reply…' }), 'Writing a reply…', 'legado sem código não é reescrito aqui')
  assert.equal(activityText(t, 'pt-BR', { activity: 'Editando loja.ts', activityCode: null }), 'Editando loja.ts')
  assert.equal(activityText(t, 'pt-BR', { activity: '', activityCode: null }), '')
  assert.equal(activityText(t, 'pt-BR', { activity: null }), '')
  const ruins = [{ kind: 'flying' }, { kind: 'downloading' }, { kind: 'tool', title: '' }, { kind: 'thinking', member: '' }, { kind: 'thinking', member: 3 }, 'typing', 7, []]
  for (const activityCode of ruins) {
    const bot = normalizeBot({ id: 'a', status: 'working', activity: 'Byte: Edit loja.ts', activityCode: activityCode as never })
    assert.equal(activityText(t, 'pt-BR', bot), 'Byte: Edit loja.ts', JSON.stringify(activityCode))
  }
  assert.equal(activityText(t, 'pt-BR', { activity: 'Typing…', activityCode: { kind: 'typing' } }), 'Escrevendo…', 'código conhecido vence o legado')
})

test('troca de idioma ao vivo: o mesmo bot muda de texto com o tradutor, sem reler o Hub', () => {
  const bot = normalizeBot({ id: 'a', status: 'working', activity: 'Editing a.ts', activityCode: { kind: 'needs_approval', title: 'Edit a.ts' } as ActivityCode })
  const porIdioma = APP_LANGUAGES.map((lang) => activityText(tradutor(lang), lang, bot))
  assert.equal(new Set(porIdioma).size, 6)
  assert.equal(activityText(tradutor('pt-BR'), 'pt-BR', bot), traduzir('pt-BR', 'sessions.detail.needs.permission', { texto: 'Edit a.ts' }))
  // A espera também: a hora acompanha o idioma.
  const espera = normalizeBot({ id: 'b', status: 'waiting', resumeAt: Date.UTC(2026, 10, 11, 15, 30), activity: 'Aguardando · retomada às 12:30', activityCode: { kind: 'subscription_wait' } as ActivityCode })
  assert.notEqual(activityText(tradutor('pt-BR'), 'pt-BR', espera), activityText(tradutor('en'), 'en', espera))
  // Atividade livre não muda com o idioma: é nome real de ferramenta.
  const livre = normalizeBot({ id: 'c', status: 'working', activity: 'Editando loja.ts' })
  for (const lang of APP_LANGUAGES) assert.equal(activityText(tradutor(lang), lang, livre), 'Editando loja.ts')
})

test('needs_approval é traduzido onde a atividade é exibida, inclusive com o bot pedindo você', () => {
  const pedindo = normalizeBot({ id: 'a', status: 'needsInput', activity: 'Needs approval: Edit a.ts', activityCode: { kind: 'needs_approval', title: 'Edit a.ts' } as ActivityCode })
  for (const lang of APP_LANGUAGES) {
    assert.equal(activityText(tradutor(lang), lang, pedindo), traduzir(lang, 'sessions.detail.needs.permission', { texto: 'Edit a.ts' }), lang)
  }
  assert.equal(activityText(tradutor('pt-BR'), 'pt-BR', pedindo), 'Pede aprovação: Edit a.ts')
})

test('MX-253: a linha de atividade da ferramenta sai em linguagem simples nos 6 idiomas e guarda o título original', () => {
  const esperado: Record<AppLanguage, [string, string]> = {
    'pt-BR': ['Lendo src/pace.js', 'Trabalhando npm test'],
    en: ['Reading src/pace.js', 'Working on npm test'],
    es: ['Leyendo src/pace.js', 'Trabajando en npm test'],
    fr: ['Lecture : src/pace.js', 'Travail sur : npm test'],
    'zh-CN': ['正在读取 src/pace.js', '正在处理 npm test'],
    'ar-AE': ['⁨جارٍ القراءة⁩: ⁨src/pace.js⁩', '⁨جارٍ العمل على⁩: ⁨npm test⁩'],
  }
  for (const lang of APP_LANGUAGES) {
    const t = tradutor(lang)
    const leitura = normalizeBot({ id: 'a', status: 'working', activity: 'Read src/pace.js', activityCode: { kind: 'tool', title: 'Read src/pace.js' } as ActivityCode })
    const comando = normalizeBot({ id: 'b', status: 'working', activity: '`npm test`', activityCode: { kind: 'tool', title: '`npm test`' } as ActivityCode })
    assert.equal(activityText(t, lang, leitura), esperado[lang][0], lang)
    assert.equal(activityText(t, lang, comando), esperado[lang][1], lang)
    assert.equal(activityOriginal(leitura), 'Read src/pace.js')
    assert.equal(activityOriginal(comando), '`npm test`')
  }
  // Crases não provam comando: um caminho entre crases também fica com o verbo neutro (revisão do #757).
  for (const lang of APP_LANGUAGES) {
    const caminho = normalizeBot({ id: 'd', status: 'working', activity: '`src/app.ts`', activityCode: { kind: 'tool', title: '`src/app.ts`' } as ActivityCode })
    assert.equal(activityText(tradutor(lang), lang, caminho), stepText(tradutor(lang), { title: '`src/app.ts`', kind: 'other' }), lang)
    assert.ok(!activityText(tradutor(lang), lang, caminho).includes(tradutor(lang)('conversa.cartoes.passos.acao.rodar')), lang)
  }
  // Sem código (Hub antigo) ou código que não é de ferramenta: sem título original.
  assert.equal(activityOriginal(normalizeBot({ id: 'c', status: 'working', activity: 'Read a.ts' })), '')
  assert.equal(activityOriginal({ activityCode: { kind: 'thinking' } }), '')
})
