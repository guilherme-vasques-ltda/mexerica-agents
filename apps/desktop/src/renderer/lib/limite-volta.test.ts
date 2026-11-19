// MX-245: a hora em que o limite volta, com relógio e fuso injetados (nada lê o relógio nem o fuso do computador).
import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { tradutor } from './fork-i18n/index.ts'
import { paradaPeloLimite, voltaCurta, voltaDaJanela, voltaEm } from './limite-volta.ts'
import { APP_LANGUAGES } from '../../shared/fork-idioma.ts'

const SP = 'America/Sao_Paulo' // UTC-3 o ano todo (sem horário de verão)
const local = (iso: string) => Date.parse(`${iso}-03:00`)
// 11/11/2026 é quarta-feira, 15:00 em São Paulo.
const AGORA = local('2026-11-11T15:00:00')
const pt = tradutor('pt-BR')
const en = tradutor('en')

test('hoje: só a hora, no formato do idioma', () => {
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-11T17:30:00'), AGORA, SP), 'volta às 17:30')
  assert.equal(voltaEm(en, 'en', local('2026-11-11T17:30:00'), AGORA, SP), 'back at 05:30 PM')
  assert.equal(voltaEm(tradutor('zh-CN'), 'zh-CN', local('2026-11-11T17:30:00'), AGORA, SP), '今天 17:30 恢复')
})

test('amanhã: dia civil seguinte, mesmo a poucos minutos (virada de dia)', () => {
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-12T09:00:00'), AGORA, SP), 'volta amanhã às 09:00')
  const quaseMeiaNoite = local('2026-11-11T23:55:00')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-12T00:10:00'), quaseMeiaNoite, SP), 'volta amanhã às 00:10')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-11T23:59:00'), quaseMeiaNoite, SP), 'volta às 23:59')
})

test('até 6 dias: o dia da semana abreviado; de 7 dias em diante: a data', () => {
  // O ponto da abreviação fica (CLDR do idioma pede): tirá-lo trocava "jeu." (quinta, em francês) pela palavra "jeu"
  // ("jogo"). "seg."/"ter." (pt-BR) e "jeu." (fr, no teste dos 6 idiomas abaixo) saem como o Intl formata.
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-16T09:00:00'), AGORA, SP), 'volta seg. às 09:00')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-17T09:00:00'), AGORA, SP), 'volta ter. às 09:00')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-18T09:00:00'), AGORA, SP), 'volta 18/11 às 09:00')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-12-25T09:00:00'), AGORA, SP), 'volta 25/12 às 09:00')
  assert.equal(voltaEm(en, 'en', local('2026-11-16T09:00:00'), AGORA, SP), 'back Mon at 09:00 AM')
  assert.equal(voltaEm(en, 'en', local('2026-11-18T09:00:00'), AGORA, SP), 'back 11/18 at 09:00 AM')
})

test('francês: "jeu." (quinta) mantém o ponto — sem ele vira "jeu", a palavra "jogo"', () => {
  const segunda = local('2026-11-09T08:00:00') // quinta (12/11) cai 3 dias depois, dentro da forma "semana"
  const fr = voltaEm(tradutor('fr'), 'fr', local('2026-11-12T09:00:00'), segunda, SP)!
  assert.match(fr, /\bjeu\./)
  assert.doesNotMatch(fr, /\bjeu\b(?!\.)/, 'nunca "jeu" sem o ponto (seria a palavra "jogo", não a abreviação)')
})

test('fuso: o mesmo instante cai em outro dia e hora conforme o fuso de quem vê', () => {
  const instante = Date.parse('2026-11-12T01:30:00Z') // 22:30 de quarta em São Paulo, 10:30 de quinta em Tóquio
  const agora = Date.parse('2026-11-11T18:00:00Z') // 15:00 de quarta em SP, 03:00 de quinta em Tóquio
  assert.equal(voltaEm(pt, 'pt-BR', instante, agora, SP), 'volta às 22:30')
  assert.equal(voltaEm(pt, 'pt-BR', instante, agora, 'Asia/Tokyo'), 'volta às 10:30')
  // Em Tóquio o "agora" já é quinta, então 10:30 é hoje; num fuso onde "agora" é quarta e o prazo é quinta, é amanhã.
  assert.equal(voltaEm(pt, 'pt-BR', instante, agora, 'UTC'), 'volta amanhã às 01:30')
})

test('instante no passado: não diz que volta; diz que o prazo informado passou', () => {
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-11T14:00:00'), AGORA, SP), 'o prazo informado já passou (14:00)')
  assert.equal(voltaEm(pt, 'pt-BR', local('2026-11-10T09:00:00'), AGORA, SP), 'o prazo informado já passou (10/11, 09:00)')
})

test('sem instante válido: nada (nunca chuta)', () => {
  for (const v of [null, undefined, NaN, Infinity]) assert.equal(voltaEm(pt, 'pt-BR', v, AGORA, SP), null)
})

test('árabe: hora e data entram isoladas da esquerda para a direita (LRI…PDI); fora do árabe, sem marcas', () => {
  const ar = voltaEm(tradutor('ar-AE'), 'ar-AE', local('2026-11-12T09:00:00'), AGORA, SP)!
  assert.match(ar, /غدًا/)
  assert.match(ar, /⁦[^⁩]+⁩/)
  const data = voltaEm(tradutor('ar-AE'), 'ar-AE', local('2026-11-18T09:00:00'), AGORA, SP)!
  assert.equal((data.match(/⁦/g) ?? []).length, 2, 'data e hora, cada uma isolada')
  assert.doesNotMatch(voltaEm(pt, 'pt-BR', local('2026-11-12T09:00:00'), AGORA, SP)!, /[⁦-⁩]/)
})

test('árabe: hora sai em 24h (sem ص/م, que embaralhava com o isolamento) e a data não inverte dia/mês', () => {
  const hoje = voltaEm(tradutor('ar-AE'), 'ar-AE', local('2026-11-11T21:30:00'), AGORA, SP)!
  assert.match(hoje, /21:30/)
  assert.doesNotMatch(hoje, /[صم]/, 'h23 tira o marcador de período, que invertia a leitura')
  const comData = voltaEm(tradutor('ar-AE'), 'ar-AE', local('2026-11-18T09:00:00'), AGORA, SP)!
  assert.match(comData, /18\/11/, 'dia antes do mês, não "1811/"')
  assert.doesNotMatch(comData, /[‎‏]/, 'sem marca bidi do Intl sobrando dentro do isolamento')
})

test('os 6 idiomas: toda forma sai preenchida, sem parâmetro solto', () => {
  const instantes = [local('2026-11-11T17:30:00'), local('2026-11-12T09:00:00'), local('2026-11-16T09:00:00'), local('2026-11-25T09:00:00'), local('2026-11-11T14:00:00')]
  for (const idioma of APP_LANGUAGES) {
    const t = tradutor(idioma)
    const frases = instantes.map((i) => voltaEm(t, idioma, i, AGORA, SP)!)
    frases.push(voltaDaJanela(t, idioma, { instante: null, texto: 'Sep 26 at 12pm (Asia/Taipei)' }, AGORA, SP), voltaDaJanela(t, idioma, { instante: null, texto: null }, AGORA, SP))
    frases.push(paradaPeloLimite(t, idioma, { resumeAt: instantes[1] }, false, AGORA, SP)!, paradaPeloLimite(t, idioma, {}, false, AGORA, SP)!)
    for (const f of frases) assert.ok(f && !/[{}]/.test(f), `${idioma}: ${f}`)
    assert.equal(new Set(frases).size, frases.length, `${idioma}: frases distintas`)
  }
})

test('painel: texto do provedor como veio; sem nada, diz que não informa', () => {
  assert.equal(voltaDaJanela(pt, 'pt-BR', { instante: local('2026-11-11T17:30:00'), texto: 'ignorado' }, AGORA, SP), 'volta às 17:30')
  assert.equal(voltaDaJanela(pt, 'pt-BR', { instante: null, texto: 'whenever it resets (UTC)' }, AGORA, SP), 'volta: whenever it resets (UTC)')
  assert.equal(voltaDaJanela(pt, 'pt-BR', { instante: null, texto: '  ' }, AGORA, SP), 'o provedor não informa quando volta')
  assert.equal(voltaDaJanela(en, 'en', { instante: null, texto: null }, AGORA, SP), 'the provider doesn’t say when it’s back')
  const ar = voltaDaJanela(tradutor('ar-AE'), 'ar-AE', { instante: null, texto: 'Sep 26 at 12pm' }, AGORA, SP)
  assert.ok(ar.includes('⁦Sep 26 at 12pm⁩'), 'texto do provedor isolado em árabe')
})

test('conversa: "Parou pelo limite do plano; volta às HH:MM" só na espera automática', () => {
  const at = local('2026-11-11T17:30:00')
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: at }, false, AGORA, SP), 'Parou pelo limite do plano; volta às 17:30')
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: local('2026-11-12T09:00:00'), snoozed: true, autoResume: true }, false, AGORA, SP), 'Parou pelo limite do plano; volta amanhã às 09:00')
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: null }, false, AGORA, SP), 'Parou pelo limite do plano; o provedor não informou quando volta')
  assert.equal(paradaPeloLimite(pt, 'pt-BR', {}, false, AGORA, SP), 'Parou pelo limite do plano; o provedor não informou quando volta')
  assert.equal(paradaPeloLimite(en, 'en', { resumeAt: at }, false, AGORA, SP), 'Stopped at the plan limit; back at 05:30 PM')
  // O texto do Hub diz o estado exato nestes: cancelada, retomada desligada, prazo chegou, pausada.
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: at, cancelled: true }, false, AGORA, SP), null)
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: at, autoResume: false }, false, AGORA, SP), null)
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: at, ready: true }, false, AGORA, SP), null)
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: at }, true, AGORA, SP), null)
  // Espera vencida pelo relógio (Hub antigo sem `ready`, ou antes do refresh): preserva o texto do Hub.
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: local('2026-11-11T14:59:00') }, false, AGORA, SP), null)
  assert.equal(paradaPeloLimite(pt, 'pt-BR', { resumeAt: AGORA }, false, AGORA, SP), null)
  assert.notEqual(paradaPeloLimite(pt, 'pt-BR', { resumeAt: AGORA + 60_000 }, false, AGORA, SP), null)
})

test('resetDescription: só o instante informado vira hora; o texto do provedor sai como veio', async () => {
  const raiz = new URL('../../', import.meta.url).pathname
  const bundle = await build({
    stdin: { contents: "export { resetDescription } from './renderer/views/usage/usage-model'", resolveDir: raiz, loader: 'ts' },
    bundle: true, platform: 'node', format: 'cjs', write: false,
    alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
    plugins: [{ name: 'tema', setup(b) {
      b.onResolve({ filter: /\/lib\/theme$/ }, () => ({ path: 'x', namespace: 'tema' }))
      b.onLoad({ filter: /.*/, namespace: 'tema' }, () => ({ contents: 'export const isDark = () => false', loader: 'js' }))
    } }],
  })
  const module = { exports: {} as Record<string, any> }
  runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), console, Intl, Date })
  const { resetDescription } = module.exports as { resetDescription: typeof import('../views/usage/usage-model.ts').resetDescription }
  const janela = (o: object) => ({ id: 'w', label: '5-hour', percent: 10, ...o })
  const em = (h: number) => AGORA + h * 3_600_000
  // O instante vem só do `resetsAt`, conferido pelo valor (não só pelo formato), inclusive no horário de verão.
  const NY = 'America/New_York'
  const antes = Date.parse('2026-03-08T05:00:00Z') // 00:00 EST, domingo da virada
  assert.equal(resetDescription(pt, 'pt-BR', janela({ resetsAt: Date.parse('2026-03-08T07:30:00Z') }), antes, NY), 'volta às 03:30', '3:30 EDT = 07:30Z (a hora 2:xx não existe)')
  assert.equal(resetDescription(pt, 'pt-BR', janela({ resetsAt: Date.parse('2026-03-08T06:30:00Z') }), antes, NY), 'volta às 01:30', '01:30 EST = 06:30Z')
  assert.equal(resetDescription(pt, 'pt-BR', janela({ resetsAt: em(24 * 30) }), AGORA, SP), 'volta 11/12 às 15:00')
  // O texto do provedor nunca vira instante: sem ano, com fuso inválido ou na virada do horário de verão, sai como veio.
  for (const texto of ['Mar 8 at 3:30am (America/New_York)', 'Sep 26 at 12pm (Asia/Taipei)', 'Nov 3 at 1:30am (Fuso/Inexistente)', '7:30pm (Asia/Taipei)', 'quando liberar']) {
    assert.equal(resetDescription(pt, 'pt-BR', janela({ resetsText: texto }), antes, NY), `volta: ${texto}`)
  }
  assert.equal(resetDescription(pt, 'pt-BR', janela({ resetsAt: Date.parse('2026-03-08T07:30:00Z'), resetsText: 'Mar 8 at 3:30am (America/New_York)' }), antes, NY), 'volta às 03:30', 'o instante vale mais que o texto')
  assert.equal(resetDescription(pt, 'pt-BR', janela({}), AGORA, SP), 'o provedor não informa quando volta')
})

test('ilha: forma curta só do instante informado; sem instante ou já passado, nada', () => {
  assert.equal(voltaCurta('pt-BR', local('2026-11-11T17:30:00'), AGORA, SP), '17:30')
  assert.equal(voltaCurta('pt-BR', local('2026-11-12T09:00:00'), AGORA, SP), 'amanhã 09:00')
  assert.equal(voltaCurta('en', local('2026-11-12T09:00:00'), AGORA, SP), 'tomorrow 09:00 AM')
  assert.equal(voltaCurta('pt-BR', local('2026-11-16T09:00:00'), AGORA, SP), 'seg. 09:00')
  assert.equal(voltaCurta('pt-BR', local('2026-11-18T09:00:00'), AGORA, SP), '18/11, 09:00')
  assert.equal(voltaCurta('pt-BR', local('2026-11-11T14:00:00'), AGORA, SP), null)
  assert.equal(voltaCurta('pt-BR', null, AGORA, SP), null)
  assert.ok(voltaCurta('ar-AE', local('2026-11-12T09:00:00'), AGORA, SP)!.includes('\u2066'))
})
