import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { pareceTelefone, telefoneDaConversa, type ConversaWa } from './caixa.ts'
import { numeroAtivo } from './numero-bot.ts'
import { nomeEscondido, rotuloConversa, rotuloPrevia } from './contato-rotulos.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

// D387 (D70): contatos e grupos sem nome salvo e a conta de saída do cartão passam pelo NumeroBot. Escondido = sem o
// número real no HTML, no nome acessível e no texto copiável; fora de vista, nada revelado. Números fictícios.
const SEM_NOME = '+5511900000107'
const REMETENTE = '+5511900000301'
const traduzirLivre = traduzir as unknown as (idioma: string, chave: string, params?: Record<string, string>) => string
const t = Object.assign((k: string, p?: Record<string, string>) => traduzirLivre('pt-BR', k, p), { plural: () => '' }) as never
const vaza = (html: string, phone: string) => {
  const d = phone.replace(/\D/g, '')
  return [d, d.slice(2), d.slice(-4), `${d.slice(4, 9)}-${d.slice(9)}`].filter((x) => html.includes(x))
}
const last = (o: Partial<ConversaWa['last']> = {}): ConversaWa['last'] => ({ senderName: 'Paulo', senderContact: true, body: 'oi', media: null, call: null, sentAt: 1, offline: false, ...o })
const conv = (o: Partial<ConversaWa>): ConversaWa => ({ whatsappChatId: 'x', kind: 'individual', name: null, phone: SEM_NOME, pinned: false, unread: 0, last: last(), ...o })

test('forma de telefone e conversa sem nome salvo', () => {
  for (const s of ['+55 11 90000-0107', '5511900000107', '+1 (202) 555-0199']) assert.equal(pareceTelefone(s), true, s)
  for (const s of ['Carlos', 'prosa@exemplo.com', '1234', '', null]) assert.equal(pareceTelefone(s), false, String(s))
  assert.equal(telefoneDaConversa(conv({})), SEM_NOME)
  assert.equal(telefoneDaConversa(conv({ name: 'Carlos' })), null)
  assert.equal(telefoneDaConversa(conv({ kind: 'group', name: null, phone: null })), null)
})

test('nome acessível da linha e da conversa: sem o número, "Número escondido, termina em NN"', () => {
  assert.equal(rotuloConversa(t, conv({})), 'Número escondido, termina em 07')
  assert.equal(rotuloConversa(t, conv({ name: 'Carlos' })), 'Carlos')
  const g = conv({ kind: 'group', name: 'Clientes VIP', phone: null, last: last({ senderName: REMETENTE, senderContact: false, body: 'Fechado' }) })
  assert.equal(rotuloPrevia(t, g), 'Número escondido, termina em 01: Fechado')
  assert.equal(rotuloPrevia(t, conv({ kind: 'group', name: 'G', phone: null, last: last({ senderName: 'Marina', senderContact: false }) })), '~Marina: oi')
  for (const s of [rotuloConversa(t, conv({})), rotuloPrevia(t, g)]) assert.deepEqual([...vaza(s, SEM_NOME), ...vaza(s, REMETENTE)], [])
  // A confirmação de "Apagar esta conversa" (R8) leva a máscara, nunca o número.
  const msg = traduzirLivre('pt-BR', 'whatsapp.conversa.apagarContato', { nome: nomeEscondido(t, conv({})) })
  assert.match(msg, /^Apaga as mensagens com \+55 •• ••••-••07 guardadas/)
  assert.deepEqual(vaza(msg, SEM_NOME), [])
  assert.equal(nomeEscondido(t, conv({ name: 'Carlos' })), 'Carlos')
})

test('fora de vista (tela grande por cima, painel fechado) ou sem olhinho: nunca revelado', () => {
  assert.equal(numeroAtivo(undefined, true, true), true)
  assert.equal(numeroAtivo(true, false, true), false, 'superfície escondida por CSS')
  assert.equal(numeroAtivo(false, true, true), false, 'painel fora de vista')
  assert.equal(numeroAtivo(true, true, false), false, 'linha da lista: só a máscara')
})

test('a ligação: lista, cabeçalho, remetente, prévia, cartão e a conversa escondida pela tela grande', () => {
  const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
  const mini = fonte('./MiniWhatsapp.tsx')
  assert.match(mini, /<span className="nm"><NomeConversa c=\{c\} olho=\{false\} \/><\/span>/)
  assert.match(mini, /<span><Previa c=\{c\} \/><\/span>/)
  assert.match(mini, /<NomeConversa c=\{conversa\} ativo=\{ativo\} \/>/)
  assert.match(mini, /<Remetente name=\{m\.from\.name\} phone=\{m\.from\.phone\} contact=\{m\.from\.contact\} ativo=\{ativo\} \/>/)
  assert.match(mini, /onVoltar=\{voltar\} ativo=\{controle\.painelVisivel\} \/>/)
  assert.doesNotMatch(mini, /nomeDa\(|remetente\(m\.from/, 'nenhum nome ou remetente cru na tela')
  assert.match(mini, /\{ nome: nomeEscondido\(t, conversa\) \}/, 'confirmação de apagar conversa')
  assert.match(fonte('../conector-card/ConectorCard.tsx'), /<IdentidadeConta identidade=\{apresentacao\.conta\.identidade\} \/>/)
  assert.match(fonte('../../thread/ThreadView.tsx'), /<SuperficieAVista\.Provider value=\{!telaOn\}>/)
  assert.match(fonte('./TelaGrande.tsx'), /<SuperficieAVista\.Provider value>/)
})

test('render real: sem nome, remetente só número, prévia e conta de saída escondidos; e-mail como veio', async () => {
  const bundle = await build({
    stdin: {
      contents: `export { NomeConversa, Remetente, Previa, IdentidadeConta } from './views/fork/whatsapp/contato';
        export { Linha } from './views/fork/whatsapp/MiniWhatsapp';
        export { PluginsPage } from './views/fork/whatsapp/PluginsPage';
        export { SuperficieAVista } from './views/fork/whatsapp/superficie';
        export { createElement } from 'react';
        export { renderToStaticMarkup } from 'react-dom/server';`,
      resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
    },
    bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
    plugins: [{ name: 'icone', setup(b) {
      b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
      b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    } }, { name: 'idioma', setup(b) {
      // useT real usa useSyncExternalStore, que não renderiza no servidor: aqui, o mesmo traduzir em pt-BR.
      b.onResolve({ filter: /fork-i18n\/estado$/ }, () => ({ path: 'idioma', namespace: 'idioma' }))
      b.onLoad({ filter: /.*/, namespace: 'idioma' }, () => ({
        contents: `import { traduzir, traduzirPlural, superficieIdioma } from './lib/fork-i18n/index.ts'; export { superficieIdioma }; export const useIdioma = () => 'pt-BR'; export function useT() { const t = (k, p) => traduzir('pt-BR', k, p); t.plural = (b, n, p) => traduzirPlural('pt-BR', b, n, p); return t }`,
        loader: 'ts', resolveDir: new URL('../../../', import.meta.url).pathname,
      }))
    } }],
    loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
    define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
  })
  // Janela mínima, como em genui/choice-card.test.ts: os módulos compartilhados leem preferências ao carregar.
  const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
  const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
  const storage = { getItem: () => null, setItem() {}, removeItem() {} }
  const document = { documentElement: { lang: '', dir: '', style: {}, dataset: {} }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {} }
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage, console, setTimeout, clearTimeout, TextEncoder, URLSearchParams, queueMicrotask })
  const m = module.exports as Record<string, unknown> & { createElement: typeof import('react').createElement; renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup }
  const html = (tipo: string, props: object) => m.renderToStaticMarkup(m.createElement(m[tipo] as never, props as never))
  const nome = html('NomeConversa', { c: conv({}) })
  assert.match(nome, /\+55 •• ••••-••07/)
  assert.match(nome, /aria-label="Número escondido, termina em 07"/)
  assert.match(nome, /aria-label="Mostrar número"/, 'cabeçalho com olhinho')
  assert.deepEqual(vaza(nome, SEM_NOME), [])
  const linha = html('NomeConversa', { c: conv({}), olho: false })
  assert.doesNotMatch(linha, /<button/, 'dentro do botão da linha: só a máscara')
  // R1-01 do #587: nome, remetente, prévia e e-mail isolados; o número em LTR.
  assert.equal(html('NomeConversa', { c: conv({ name: 'Carlos (exemplo)' }) }), '<bdi dir="auto">Carlos (exemplo)</bdi>')
  const quem = html('Remetente', { name: REMETENTE, phone: REMETENTE, contact: false })
  assert.match(quem, /^~<span class="wa-num">/)
  assert.deepEqual(vaza(quem, REMETENTE), [])
  assert.equal(html('Remetente', { name: 'Marina', contact: false }), '<bdi dir="auto">~Marina</bdi>')
  const previa = html('Previa', { c: conv({ kind: 'group', name: 'G', phone: null, last: last({ senderName: REMETENTE, senderContact: false, body: 'Fechado' }) }) })
  assert.match(previa, /<bdi dir="ltr">\+55 •• ••••-••01<\/bdi><\/span><\/span>: <bdi dir="auto">Fechado<\/bdi>$/)
  assert.deepEqual(vaza(previa, REMETENTE), [])
  const conta = html('IdentidadeConta', { identidade: '+55 11 90000-0001' })
  assert.match(conta, /\+55 •• ••••-••01/)
  assert.deepEqual(vaza(conta, '+5511900000001'), [])
  assert.equal(html('IdentidadeConta', { identidade: 'prosa@exemplo.com' }), '<bdi dir="ltr">prosa@exemplo.com</bdi>')
  // D387-SOL-01: a LINHA inteira da lista (botão, aria-label, data-*, title), com o olhinho fechado: nem o telefone cru,
  // nem o JID, nem o formatado; nenhum pedaço além dos 2 últimos dígitos. Contato com nome: o número dele também não.
  const JID = '5511900000107@s.whatsapp.net'
  for (const c of [conv({ whatsappChatId: JID }), conv({ whatsappChatId: '5511900000102@s.whatsapp.net', name: 'Carlos (exemplo)', phone: '+5511900000102' }),
    conv({ whatsappChatId: '120363000000002@g.us', kind: 'group', name: 'Clientes VIP', phone: null, last: last({ senderName: REMETENTE, senderContact: false, body: 'Fechado' }) })]) {
    const row = html('Linha', { c, sel: false, onClick() {} })
    assert.ok(!row.includes('@s.whatsapp.net') && !row.includes(c.whatsappChatId), `sem JID: ${row}`)
    assert.match(row, /data-chat="c\d+"/)
    // R1-01 do #587: a hora da linha em LTR isolado (a formatação continua no idioma do app).
    assert.match(row, /class="tm[^"]*"><bdi dir="ltr">/)
    for (const phone of [SEM_NOME, '+5511900000102', REMETENTE]) assert.deepEqual(vaza(row, phone), [], `${phone} em ${row}`)
  }
  // R1-02 do #587: sem computador, o erro sai dentro da superfície do idioma do app (o <body> é o limite pt-BR/LTR).
  const semComputador = html('PluginsPage', { store: null, openVoice() {} })
  assert.match(semComputador, /<div class="idioma-superficie" lang="pt-BR" dir="ltr"[^>]*>[\s\S]*Nenhum computador conectado/)
  // Conversa escondida pela tela grande: também só a máscara.
  const oculta = m.renderToStaticMarkup(m.createElement((m.SuperficieAVista as { Provider: never }).Provider, { value: false } as never, m.createElement(m.IdentidadeConta as never, { identidade: '+55 11 90000-0001' } as never)))
  assert.deepEqual(vaza(oculta, '+5511900000001'), [])
})

test('D401: revelado, o número do cabeçalho aparece inteiro (sem corte) e o selo encolhe; escondido, corta no começo', () => {
  const css = readFileSync(new URL('./mini-whatsapp.css', import.meta.url), 'utf8')
  const regra = (sel: string) => css.slice(css.indexOf(`${sel} {`)).split('}')[0]!
  const revelado = regra('.wa-cvh-t .wa-num-t:not(.escondido)')
  assert.match(revelado, /overflow: visible/)
  assert.match(revelado, /text-overflow: clip/)
  assert.match(revelado, /flex: none/, 'não encolhe: nenhum dígito cortado')
  assert.match(regra(".wa-cvh:has(.wa-cvh-t .wa-num-olho[aria-pressed='true']) .wa-fora"), /font-size: 0/, 'o selo vira só o escudo')
  assert.match(regra('.wa-cvh-t .wa-num-t'), /text-overflow: ellipsis/, 'escondido continua cortando no começo')
})
