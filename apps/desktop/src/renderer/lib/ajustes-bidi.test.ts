import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { readFileSync } from 'node:fs'
import { build, type Plugin } from 'esbuild'
import { direcaoDoNome, isolado, ltr } from '../../shared/bidi.ts'
import { COMANDO_INSTALAR_HOST, motivoSSH, textoSSH } from '../../shared/ssh-startup.ts'
import { CATALOGOS } from './fork-i18n/index.ts'

// Lição da R1 do #565 na fatia 4 (D454): comando, caminho, host e código ficam da esquerda para a direita e isolados
// do texto em volta; nome do usuário isolado na direção dele. Em árabe, sem isso, o bidi troca a ordem do que a pessoa
// confere (e aprova).

const LRI = '⁦', FSI = '⁨', PDI = '⁩'

test('bidi: ltr isola em LRI…PDI e isolado em FSI…PDI', () => {
  assert.equal(ltr('a/b'), `${LRI}a/b${PDI}`)
  assert.equal(isolado('مكتب'), `${FSI}مكتب${PDI}`)
})

test('SSH em árabe: host e comando vêm isolados da esquerda para a direita, o detalhe isolado na direção dele', () => {
  const recusou = textoSSH(motivoSSH('ajustes.ssh.erro.recusou', { host: 'mini.local', comando: 'ssh mini.local' }).motivo, 'ar-AE')
  assert.ok(recusou.includes(ltr('mini.local')))
  assert.ok(recusou.includes(ltr('ssh mini.local')))
  const identidade = textoSSH(motivoSSH('ajustes.ssh.erro.identidade', { host: 'mini', comando: COMANDO_INSTALAR_HOST, detalhe: 'exit 127' }).motivo, 'ar-AE')
  assert.ok(identidade.includes(ltr(COMANDO_INSTALAR_HOST)))
  assert.ok(identidade.includes(isolado('exit 127')))
  // O texto que o main grava em `message` (inglês base da barra e dos chamadores antigos) não leva isolantes.
  assert.doesNotMatch(motivoSSH('ajustes.ssh.erro.recusou', { host: 'mini', comando: 'ssh mini' }).texto, /[⁦-⁩]/)
})

test('fatia 4: nenhum texto de ajustes.* nos 6 catálogos traz comando ou caminho dentro da frase', () => {
  const codigo = /~\/|\/etc\/|`|ssh-keygen|\.local\/bin/
  const achados = Object.entries(CATALOGOS).flatMap(([idioma, catalogo]) =>
    Object.entries(catalogo).filter(([chave, valor]) => chave.startsWith('ajustes.') && codigo.test(String(valor))).map(([chave]) => `${idioma}: ${chave}`))
  assert.deepEqual(achados, [])
})

// Renderiza no servidor o cartão de aprovação e a confirmação da chave SSH reais, em árabe. Os hooks de estado do app
// (`useSyncExternalStore` sem snapshot de servidor) ganham, só no bundle deste teste, o próprio `getSnapshot` também no
// servidor.
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'react-servidor' ? undefined : { path: 'react', namespace: 'react-servidor' }))
    b.onLoad({ filter: /.*/, namespace: 'react-servidor' }, () => ({
      contents: `const R = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))});
        module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
// R1-01 do #568: menu e diálogo abrem num portal no body (limite legado pt-BR/ltr). Só neste bundle: o portal
// renderiza no lugar, o menu do botão já abre e a posição da âncora já está medida (sem DOM no Node).
const portaisNoLugar: Plugin = {
  name: 'portais-no-lugar',
  setup(b) {
    b.onResolve({ filter: /^react-dom$/ }, (args) => (args.namespace === 'react-dom-teste' ? undefined : { path: 'react-dom', namespace: 'react-dom-teste' }))
    b.onLoad({ filter: /.*/, namespace: 'react-dom-teste' }, () => ({
      contents: `const D = require(${JSON.stringify(createRequire(import.meta.url).resolve('react-dom'))});
        module.exports = { ...D, createPortal: (filhos) => filhos };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
    const trocar = (arquivo: RegExp, de: string, para: string) => b.onLoad({ filter: arquivo }, (args) => {
      const fonte = readFileSync(args.path, 'utf8')
      if (!fonte.includes(de)) throw new Error(`teste desatualizado: ${de}`)
      return { contents: fonte.replace(de, para), loader: 'tsx' }
    })
    trocar(/components[\\/]Controls\.tsx$/, 'const [open, setOpen] = useState(false)', 'const [open, setOpen] = useState(true)')
    trocar(/components[\\/]Overlay\.tsx$/, 'const [rect, setRect] = useState<DOMRect | null>(null)', 'const [rect, setRect] = useState<DOMRect | null>({ top: 0, left: 0, bottom: 20, right: 100, width: 100, height: 20 } as DOMRect)')
  },
}
const bundle = await build({
  stdin: {
    contents: `export { ApprovalSheet } from './views/settings/ApprovalSheet';
      export { SSHRow } from './views/settings/SSHRow';
      export { AppContext } from './store/context';
      export { ComputerFilterHeader } from './views/ComputerFilterHeader';
      export { ssh } from './views/settings/ssh-model';
      export { Dialog } from './components/Overlay';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor, portaisNoLugar],
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const salvo = new Map([['appLanguage', JSON.stringify('ar-AE')]])
const storage = { getItem: (k: string) => salvo.get(k) ?? null, setItem() {}, removeItem() {} }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = {
  codync: { platform: 'darwin', appScheme: 'donnabot', ssh: {} },
  addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage,
  innerWidth: 1200, innerHeight: 800,
}
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'ar-AE' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  ApprovalSheet: typeof import('../views/settings/ApprovalSheet').ApprovalSheet
  SSHRow: typeof import('../views/settings/SSHRow').SSHRow
  AppContext: typeof import('../store/context').AppContext
  ComputerFilterHeader: typeof import('../views/ComputerFilterHeader').ComputerFilterHeader
  ssh: typeof import('../views/settings/ssh-model').ssh
  Dialog: typeof import('../components/Overlay').Dialog
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

test('cartão de aprovação em árabe: superfície rtl, código e e-mail ltr isolados, nomes em <bdi>', () => {
  const app = { subscribe: () => () => {}, getVersion: () => 0 }
  const approval = { request: { deviceName: 'آيفون سارة', email: 'sara@example.com', code: '482913', platform: 'ios' }, store: { hostName: 'Mac mini' } }
  const html = m.renderToStaticMarkup(m.createElement(m.AppContext.Provider, { value: app as never }, m.createElement(m.ApprovalSheet, { approval: approval as never })))
  assert.match(html, /class="[^"]*settings-approval[^"]*idioma-superficie[^"]*"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  // Opção B (segurança): o Mac não exibe o código; a pessoa digita o que o aparelho mostra e Aprovar só abre se for igual.
  assert.match(html, /<input[^>]*class="[^"]*bidi-isolado[^"]*"[^>]*dir="ltr"/)
  assert.doesNotMatch(html, /482\s?913/)
  assert.match(html, /<button[^>]*disabled=""[^>]*>(?:(?!<\/button>).)*موافقة/)
  assert.match(html, /<bdi dir="ltr">sara@example\.com<\/bdi>/)
  assert.match(html, /<bdi>آيفون سارة<\/bdi>/)
  assert.match(html, /<bdi>Mac mini<\/bdi>/)
})

test('MX-149: depois de aprovar, a folha em árabe mostra o código de confirmação 3+3 em <bdi dir="ltr">, com o aparelho isolado', () => {
  const app = { subscribe: () => () => {}, getVersion: () => 0 }
  const approval = { request: { deviceName: 'آيفون سارة', code: '482913', platform: 'ios' }, store: { hostName: 'Mac mini' } }
  const html = m.renderToStaticMarkup(m.createElement(m.AppContext.Provider, { value: app as never }, m.createElement(m.ApprovalSheet, { approval: approval as never, confirmCode: '244257' })))
  assert.match(html, /class="[^"]*settings-approval[^"]*idioma-superficie[^"]*"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  assert.match(html, /<bdi dir="ltr"[^>]*class="[^"]*settings-approval-confirm[^"]*"[^>]*>244 257<\/bdi>/)
  assert.match(html, /اكتب هذا الرمز الآن على \u2068<bdi>آيفون سارة<\/bdi>\u2069/)
  assert.match(html, />تم<\/button>/)
  assert.doesNotMatch(html, /<input/)
})

test('MX-149: antes do resultado approved a folha não mostra código de confirmação', () => {
  const app = { subscribe: () => () => {}, getVersion: () => 0 }
  const approval = { request: { deviceName: 'iPhone', code: '482913', platform: 'ios' }, store: { hostName: 'Mac mini' } }
  const html = m.renderToStaticMarkup(m.createElement(m.AppContext.Provider, { value: app as never }, m.createElement(m.ApprovalSheet, { approval: approval as never })))
  assert.doesNotMatch(html, /settings-approval-confirm|482\s?913/)
})

test('MX-149: as chaves novas de ajustes.pedido existem nos 6 idiomas, sem jargão e com {aparelho}', async () => {
  const chaves = ['pronto', 'digiteNoAparelho', 'codigoConfirmacao', 'termineConectar', 'concluido']
  for (const lang of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE']) {
    const mod = await import(`./fork-i18n/${lang}/ajustes.ts`) as Record<string, Record<string, string>>
    const cat = Object.values(mod).find((v) => v && typeof v === 'object' && 'ajustes.pedido.titulo' in v)!
    for (const k of chaves) {
      const v = cat[`ajustes.pedido.${k}`]
      assert.ok(v && v.length > 0, `${lang}: ${k}`)
      assert.doesNotMatch(v, /\bSAS\b|transcript|hash/i, `${lang}: ${k} sem jargão`)
    }
    assert.match(cat['ajustes.pedido.digiteNoAparelho']!, /\{aparelho\}/, lang)
  }
})

test('confirmação da chave SSH em árabe: o comando e as impressões ficam ltr e isolados; o alvo vai em LRI', () => {
  const profile = { id: 'p', name: '', host: 'mini.local', user: 'sara', port: 2222, remotePort: 0, identityFile: null }
  const status = { kind: 'confirmHostKey', name: 'mini.local', fingerprints: ['SHA256:abc/def+ghi'] }
  const html = m.renderToStaticMarkup(m.createElement(m.SSHRow, { profile: profile as never, status: status as never, edit: () => {} }))
  assert.match(html, /<code class="[^"]*bidi-isolado[^"]*" dir="ltr">ssh-keygen -lf \/etc\/ssh\/ssh_host_ed25519_key\.pub<\/code>/)
  assert.match(html, /class="[^"]*bidi-isolado[^"]*"[^>]*dir="ltr"[^>]*>SHA256:abc\/def\+ghi</)
  assert.ok(html.includes(ltr('sara@mini.local:2222')))
  assert.match(html, /dir="ltr"[^>]*>mini\.local</)
})

test('R1-01: o menu de computadores em árabe declara lang/dir na raiz do portal; nome isolado, host ltr, estado traduzido', () => {
  const store = { hostName: 'مكتب سارة', shownConnection: { kind: 'online' }, connection: { kind: 'online' }, mismatch: false, restartStream() {} }
  const app = {
    subscribe: () => () => {}, getVersion: () => 0,
    computers: [{ id: 'c1', name: 'مكتب سارة' }], store: () => store, host: { state: { kind: 'running' } },
  }
  m.ssh.state = { profiles: [{ id: 'p', name: '', host: 'mini.local', user: null, port: null, remotePort: 0, identityFile: null }], status: {}, attachments: [] } as never
  const html = m.renderToStaticMarkup(m.createElement(m.AppContext.Provider, { value: app as never }, m.createElement(m.ComputerFilterHeader, { hidden: '', setHidden: () => {}, manage: () => {} })))
  assert.match(html, /<div[^>]*class="anchored[^"]*idioma-superficie[^"]*"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  assert.ok(html.includes(`${isolado('مكتب سارة')} · متصل`), 'nome isolado e estado pelo catálogo (não o connectionLabel legado)')
  assert.ok(html.includes(ltr('mini.local')), 'host do perfil SSH pendente da esquerda para a direita')
  assert.doesNotMatch(html, /Connected/)
})

test('R1-01/R1-02: o diálogo com language declara lang/dir na raiz do portal; sem ele, nada muda (limite legado)', () => {
  const props = { open: true, title: 'هل تريد إزالة؟', actions: [], onClose: () => {} }
  const traduzido = m.renderToStaticMarkup(m.createElement(m.Dialog, { ...props, cancel: 'إلغاء', language: { lang: 'ar-AE', dir: 'rtl' } }))
  assert.match(traduzido, /^<div[^>]*class="dialog[^"]*idioma-superficie"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  assert.match(traduzido, />إلغاء<\/button>/)
  const legado = m.renderToStaticMarkup(m.createElement(m.Dialog, props))
  assert.doesNotMatch(legado, /lang=|dir=|idioma-superficie/)
  assert.match(legado, />Cancelar<\/button>/)
})

test('R1-01/R1-02: as 4 confirmações de Computadores e Dispositivos passam o idioma e o Cancelar do catálogo', () => {
  const fonte = (arquivo: string) => readFileSync(new URL(`../views/settings/${arquivo}`, import.meta.url), 'utf8')
  const dialogos = ['ComputersView.tsx', 'AuthorizedDevicesSection.tsx'].flatMap((a) => fonte(a).split('<Dialog').slice(1).map((d) => d.slice(0, d.indexOf('/>'))))
  assert.equal(dialogos.length, 4)
  for (const d of dialogos) {
    assert.match(d, /cancel=\{t\('ajustes\.cancelar'\)\}/)
    assert.match(d, /language=\{superficieIdioma\(idioma\)\}/)
  }
})

test('R1 do #591: o nome do anexo no título da folha RTL fica na direção própria; sem letra forte, em LTR', () => {
  // Em ar-AE a folha é RTL; `01.png` herdaria a direção e viraria `png.01`. Com letra, `dir="auto"` resolve pelo texto.
  assert.equal(direcaoDoNome('01.png'), 'auto')
  assert.equal(direcaoDoNome('2026-11-08 foto.png'), 'auto')
  assert.equal(direcaoDoNome('صورة.png'), 'auto')
  // Só dígitos e símbolos não têm letra forte: `auto` cairia no pai RTL e inverteria os blocos ("08-11-2026").
  assert.equal(direcaoDoNome('2026-11-08'), 'ltr')
  assert.equal(direcaoDoNome('01'), 'ltr')
  assert.equal(direcaoDoNome('123_456'), 'ltr')
  // A folha usa o isolamento no título.
  const fonte = readFileSync(new URL('../views/thread/Attachments.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /title=\{<bdi dir=\{direcaoDoNome\(viewing\.name\)\}>\{viewing\.name\}<\/bdi>\}/)
})
