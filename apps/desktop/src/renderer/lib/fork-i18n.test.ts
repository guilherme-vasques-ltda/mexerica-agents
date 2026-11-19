// DON-903: tradução, fallback, formatação e a preferência persistida com troca imediata.
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'
import { CADEIA_FALLBACK, categoriasPlural, formatarData, formatarMoeda, formatarNumero, pluralEm, textoEm, traduzir, traduzirPlural } from './fork-i18n/index.ts'

test('traduz com parâmetros nos dois idiomas', () => {
  assert.equal(traduzir('pt-BR', 'idioma.passo.etapa', { atual: 1, total: 2 }), 'Passo 1 de 2')
  assert.equal(traduzir('en', 'idioma.passo.etapa', { atual: 1, total: 2 }), 'Step 1 of 2')
  assert.equal(traduzir('en', 'idioma.passo.continuar'), 'Continue')
})

test('em árabe, valores inseridos ficam isolados (bidi); em idiomas LTR, nada é acrescentado', () => {
  assert.equal(traduzir('ar-AE', 'idioma.passo.etapa', { atual: 1, total: 2 }).includes('\u20681\u2069'), true)
  assert.equal(traduzir('fr', 'idioma.passo.etapa', { atual: 1, total: 2 }).includes('\u2068'), false)
})

test('plural por Intl.PluralRules: categoria do idioma, other do idioma, depois pt-BR', () => {
  const vazio = {}
  const cats = {
    'pt-BR': { 'b.one': '{n} bot', 'b.many': '{n} de bots', 'b.other': '{n} bots' },
    en: { 'b.one': 'one bot', 'b.other': '{n} bots' },
    es: vazio, fr: vazio,
    'zh-CN': { 'b.other': '{n} 个机器人' },
    'ar-AE': { 'b.zero': 'لا بوتات', 'b.one': 'بوت واحد', 'b.two': 'بوتان', 'b.few': '{n} بوتات', 'b.many': '{n} بوتًا', 'b.other': '{n} بوت' },
  }
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 1), '1 bot')
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 1500), '1.500 bots')
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 1_000_000), '1.000.000 de bots')
  assert.equal(pluralEm(cats, 'en', 'b', 1), 'one bot')
  assert.equal(pluralEm(cats, 'en', 'b', 1500), '1,500 bots')
  assert.equal(pluralEm(cats, 'zh-CN', 'b', 1), '1 个机器人')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 0), 'لا بوتات')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 2), 'بوتان')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 3), '\u20683\u2069 بوتات')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 11), '\u206811\u2069 بوتًا')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 100), '\u2068100\u2069 بوت')
  // Sem o plural no idioma: o pt-BR, com o número no formato do idioma.
  assert.equal(pluralEm(cats, 'fr', 'b', 2), '2 bots')
  assert.deepEqual(categoriasPlural('ar-AE').obrigatorias, ['zero', 'one', 'two', 'few', 'many', 'other'])
  assert.deepEqual(categoriasPlural('zh-CN').obrigatorias, ['other'])
  assert.deepEqual(categoriasPlural('pt-BR').obrigatorias, ['one', 'many', 'other'])
  assert.deepEqual(categoriasPlural('en').obrigatorias, ['one', 'other'])
})

test('D507: .zero explícito vale com n = 0 em qualquer idioma; sem ele, segue o CLDR', () => {
  const cats = {
    'pt-BR': { 'b.zero': 'Nenhum bot', 'b.one': '{n} bot', 'b.many': '{n} de bots', 'b.other': '{n} bots' },
    en: { 'b.one': 'one bot', 'b.other': '{n} bots' },
    es: { 'b.zero': 'Ningún bot', 'b.one': '{n} bot', 'b.other': '{n} bots' },
    fr: {},
    'zh-CN': { 'b.zero': '没有机器人', 'b.other': '{n} 个机器人' },
    'ar-AE': { 'b.zero': 'لا بوتات', 'b.one': 'بوت واحد', 'b.two': 'بوتان', 'b.few': '{n} بوتات', 'b.many': '{n} بوتًا', 'b.other': '{n} بوت' },
  }
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 0), 'Nenhum bot')
  assert.equal(pluralEm(cats, 'es', 'b', 0), 'Ningún bot')
  assert.equal(pluralEm(cats, 'zh-CN', 'b', 0), '没有机器人')
  // Sem .zero no idioma: CLDR como antes (en "0 bots"), e o pt-BR só entra como fallback de quem não tem o plural.
  assert.equal(pluralEm(cats, 'en', 'b', 0), '0 bots')
  // 1 e 2 não mudam com a existência do .zero.
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 1), '1 bot')
  assert.equal(pluralEm(cats, 'pt-BR', 'b', 2), '2 bots')
  assert.equal(pluralEm(cats, 'zh-CN', 'b', 1), '1 个机器人')
  // ar-AE: o CLDR já escolhe zero; nada muda.
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 0), 'لا بوتات')
  assert.equal(pluralEm(cats, 'ar-AE', 'b', 2), 'بوتان')
  // O gate aceita .zero em todos os idiomas, mas só exige em ar-AE.
  for (const i of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) assert.equal(categoriasPlural(i).permitidas.includes('zero'), true)
  assert.equal(categoriasPlural('en').obrigatorias.includes('zero'), false)
  // Catálogo real: Nenhuma linha / No rows.
  assert.equal(traduzirPlural('pt-BR', 'conversa.cartoes.tabela.linhas', 0), 'Nenhuma linha')
  assert.equal(traduzirPlural('en', 'conversa.cartoes.tabela.linhas', 0), 'No rows')
  assert.equal(traduzirPlural('en', 'conversa.cartoes.tabela.linhas', 2), '2 rows')
  // Contagens de itens das telas já migradas: o zero tem forma própria nos 6 idiomas (ar-AE pelo CLDR).
  const zeros: [string, string][] = [['pt-BR', 'Nenhum bot'], ['en', 'No bots'], ['es', 'Ningún bot'], ['fr', 'Aucun bot'], ['zh-CN', '没有机器人'], ['ar-AE', 'لا بوتات']]
  for (const [idioma, texto] of zeros) assert.equal(traduzirPlural(idioma as 'en', 'navegacao.barra.bots', 0), texto, idioma)
  assert.equal(traduzirPlural('en', 'extensoes.instalados', 0), 'None installed')
  assert.equal(traduzirPlural('en', 'navegacao.barra.bots', 2), '2 bots')
  assert.equal(traduzirPlural('pt-BR', 'rotinas.central.contagemBots', 0), 'Nenhum bot')
  assert.equal(traduzirPlural('en', 'rotinas.central.contagemBots', 0), 'No bots')
})

test('D454/D495: idioma desconhecido cai no inglês', () => {
  assert.equal(traduzir('xx' as 'en', 'idioma.passo.continuar'), 'Continue')
})

test('D454/D495: texto sem tradução cai no inglês e, sem inglês, na fonte pt-BR', () => {
  const cats = {
    'pt-BR': { a: 'só pt', b: 'pt b', c: 'pt c' }, en: { b: 'en b', c: 'en c' },
    es: {}, fr: { c: 'fr c' }, 'zh-CN': {}, 'ar-AE': { b: '' },
  }
  assert.deepEqual(CADEIA_FALLBACK('fr'), ['fr', 'en', 'pt-BR'])
  assert.deepEqual(CADEIA_FALLBACK('en'), ['en', 'pt-BR'])
  assert.equal(textoEm(cats, 'fr', 'c'), 'fr c')
  assert.equal(textoEm(cats, 'fr', 'b'), 'en b', 'ausente em fr → en')
  assert.equal(textoEm(cats, 'ar-AE', 'b'), 'en b', 'vazio conta como ausente')
  assert.equal(textoEm(cats, 'fr', 'a'), 'só pt', 'ausente em fr e em en → pt-BR')
  assert.equal(textoEm(cats, 'en', 'a'), 'só pt', 'ausente em en → pt-BR')
  // Plural: a mesma cadeia.
  const plurais = { 'pt-BR': { 'p.one': '{n} item pt', 'p.other': '{n} itens pt' }, en: { 'q.other': '{n} q en' }, es: {}, fr: {}, 'zh-CN': {}, 'ar-AE': {} }
  assert.equal(pluralEm(plurais, 'fr', 'q', 3), '3 q en')
  assert.equal(pluralEm(plurais, 'fr', 'p', 3), '3 itens pt')
})

test('datas e números seguem o idioma; a moeda é a do dado', () => {
  const dia = Date.UTC(2026, 10, 6, 12)
  assert.equal(formatarData('pt-BR', dia, { timeZone: 'UTC' }), '06/11/2026')
  assert.equal(formatarData('en', dia, { timeZone: 'UTC' }), '11/6/2026')
  assert.equal(formatarNumero('pt-BR', 1234.5), '1.234,5')
  assert.equal(formatarNumero('en', 1234.5), '1,234.5')
  assert.equal(formatarMoeda('en', 10, 'BRL'), 'R$10.00')
  assert.equal(formatarMoeda('pt-BR', 10, 'USD').replace(/\s/g, ' '), 'US$ 10,00')
})

// lib/prefs.ts importa sem extensão (como o Vite resolve); aqui, o mesmo arquivo .ts.
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      const candidate = new URL(`${specifier}.ts`, context.parentURL)
      if (existsSync(candidate)) return next(candidate.href, context)
    }
    return next(specifier, context)
  },
})

test('D454/D495: sem escolha válida o app fala inglês; a escolha persiste e troca na hora', async () => {
  const salvo = new Map<string, string>([['appLanguage', JSON.stringify('de')]])
  const g = globalThis as Record<string, unknown>
  g.window = { addEventListener() {} }
  g.localStorage = { getItem: (k: string) => salvo.get(k) ?? null, setItem: (k: string, v: string) => void salvo.set(k, v) }
  const corpo: Record<string, string> = {}
  g.document = { documentElement: { lang: 'en', dir: '' }, body: { setAttribute: (k: string, v: string) => { corpo[k] = v } } }
  const { idiomaAtual, idiomaPref } = await import('./fork-i18n/estado.ts')
  const html = (g.document as { documentElement: { lang: string; dir: string } }).documentElement
  assert.equal(idiomaAtual(), 'en')
  assert.equal(html.lang, 'en')
  assert.equal(html.dir, 'ltr')
  // O legado ainda em português fica no limite do body, também com o padrão inglês.
  assert.deepEqual([corpo.lang, corpo.dir], ['pt-BR', 'ltr'])
  let avisos = 0
  idiomaPref.subscribe(() => avisos++)
  // Escolha salva vale: pt-BR continua pt-BR.
  idiomaPref.set('pt-BR')
  assert.equal(avisos, 1)
  assert.equal(idiomaAtual(), 'pt-BR')
  assert.equal(html.lang, 'pt-BR')
  assert.equal(salvo.get('appLanguage'), '"pt-BR"')
  idiomaPref.set('en')
  assert.equal(idiomaAtual(), 'en')
  // Árabe: o documento vira RTL; o legado ainda em português fica no limite LTR do body.
  idiomaPref.set('ar-AE')
  assert.deepEqual([html.lang, html.dir, corpo.lang, corpo.dir], ['ar-AE', 'rtl', 'pt-BR', 'ltr'])
  // E volta ao LTR sem resto.
  idiomaPref.set('pt-BR')
  assert.deepEqual([html.lang, html.dir], ['pt-BR', 'ltr'])
})
