// DON-903: o gate de idiomas (tools/textos-gate.ts) roda aqui, dentro do `npm test` do job desktop.
import assert from 'node:assert/strict'
import { fileURLToPath, pathToFileURL } from 'node:url'
import test from 'node:test'
import { execFileSync } from 'node:child_process'
import { cpSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { arquivosDoApp, areaDe, comparar, conferirArvore, conferirCatalogos, conferirModulos, envGitSanitizado, falhasEstruturais, foraDoGate, inventarioDaBase, inventariar, lerInventario, listar, naoCresceu, textosVisiveis } from '../../tools/textos-gate.ts'
import { CATALOGOS, categoriasPlural } from '../renderer/lib/fork-i18n/index.ts'
import { isAppLanguage } from './fork-idioma.ts'

const categorias = (idioma: string) => (isAppLanguage(idioma) ? categoriasPlural(idioma) : { obrigatorias: ['other'], permitidas: ['other'] })

const raiz = fileURLToPath(new URL('../..', import.meta.url))
const textos = (fonte: string, arquivo = 'x.tsx') => textosVisiveis(arquivo, fonte).map((a) => a.texto)

test('os catálogos reais têm as mesmas chaves e parâmetros', () => {
  assert.deepEqual(conferirCatalogos(CATALOGOS, categorias), [])
})

test('catálogos por área: os módulos reais estão registrados e o agregado é a união das partes', () => {
  assert.deepEqual(conferirModulos(raiz, CATALOGOS), [])
})

test('catálogos por área: duplicada entre partes, área errada, arquivo não registrado, parte esquecida e texto no caminho antigo', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-modulos-'))
  const i18n = 'src/renderer/lib/fork-i18n'
  const arq = (rel: string) => join(tmp, i18n, rel)
  const trocar = (rel: string, de: string, para: string) => {
    const s = readFileSync(arq(rel), 'utf8')
    assert.ok(s.includes(de), rel)
    writeFileSync(arq(rel), s.replace(de, para))
  }
  try {
    cpSync(join(raiz, i18n), join(tmp, i18n), { recursive: true })
    mkdirSync(join(tmp, 'tools'))
    writeFileSync(join(tmp, 'tools/javascript-legado.json'), '{ "arquivos": [] }\n')
    assert.deepEqual(conferirModulos(tmp), [])
    // Mesma chave em duas partes: o spread esconderia uma delas.
    trocar('en/tema.ts', "'tema.grupo':", "'idioma.nome': 'X',\n  'tema.grupo':")
    // Chave fora do prefixo do módulo.
    trocar('fr/contrato.ts', "'contrato.titulo':", "'delegacao.nova': 'Y',\n  'contrato.titulo':")
    // Arquivo com texto fora do registro, inclusive vazio.
    writeFileSync(arq('es/extra.ts'), "export const textos = { 'extra.a': 'Texto escondido.' }\n")
    // Parte importada mas esquecida no agregado.
    trocar('zh-CN/index.ts', '  ...tema,\n', '')
    // Texto acrescentado ao caminho antigo, que só reexporta.
    writeFileSync(arq('pt-BR.ts'), readFileSync(arq('pt-BR.ts'), 'utf8') + "export const mais = { 'idioma.novo': 'Novo texto.' }\n")
    // Módulo com outra coisa além do objeto literal.
    trocar('ar-AE/aparencia.ts', 'export const textos', "export const solto = 'Frase escondida.'\nexport const textos")
    assert.deepEqual(conferirModulos(tmp).sort(), [
      `${i18n}/ar-AE/aparencia.ts: o módulo só pode ter import type e export const textos = { … }`,
      `${i18n}/en/tema.ts: idioma.nome pertence a idioma`,
      `${i18n}/es/extra.ts: arquivo não registrado em registro.ts (todo texto de tradução fica num módulo registrado)`,
      `${i18n}/fr/contrato.ts: delegacao.nova pertence a delegacao`,
      `${i18n}/pt-BR.ts: o caminho antigo só reexporta (export { ptBR } from './pt-BR/index.ts'); texto novo vai no módulo da área`,
      `${i18n}/zh-CN/index.ts: módulo tema não entra no agregado`,
      'en: idioma.nome duplicada em idioma e tema',
    ].sort())
    // O agregado em uso tem de ser a união das partes.
    assert.deepEqual(conferirModulos(join(raiz), { ...CATALOGOS, en: { ...CATALOGOS.en, 'tema.grupo': 'Outro' } }), ['en: o agregado tem tema.grupo diferente das partes'])
  } finally {
    rmSync(tmp, { recursive: true, force: true })
  }
})

/** Copia os catálogos e acrescenta a pasta `auxiliar`, que o código carregado não declara. */
function comPastaAuxiliar(mutar: (fonte: string) => string) {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-idioma-'))
  const i18n = 'src/renderer/lib/fork-i18n'
  cpSync(join(raiz, i18n), join(tmp, i18n), { recursive: true })
  const reg = join(tmp, i18n, 'registro.ts')
  writeFileSync(reg, mutar(readFileSync(reg, 'utf8')))
  mkdirSync(join(tmp, i18n, 'auxiliar'))
  writeFileSync(join(tmp, i18n, 'auxiliar/index.ts'), "export const textos = { a: 'Texto escondido.' }\n")
  return { tmp, i18n }
}

test('comentário com auxiliar não tira a pasta da checagem e ela falha', () => {
  const { tmp, i18n } = comPastaAuxiliar((fonte) => "const exemplo = 'https://exemplo.com /* auxiliar */'\n" + fonte.replace(
    'export const IDIOMAS_CATALOGO = [',
    "export const IDIOMAS_CATALOGO = [ /* 'auxiliar' */\n  // modulo: 'auxiliar'\n  ",
  ))
  try {
    assert.ok(arquivosDoApp(tmp).includes(`${i18n}/auxiliar/index.ts`))
    assert.ok(conferirModulos(tmp).includes(`${i18n}/auxiliar: pasta que não é um idioma declarado`))
  } finally { rmSync(tmp, { recursive: true, force: true }) }
})

test('variável local falsa não declara idioma e a pasta falha', () => {
  const { tmp, i18n } = comPastaAuxiliar((fonte) => "const IDIOMAS_CATALOGO = ['auxiliar', 'pt-BR'] as const\n" + fonte)
  try {
    assert.ok(arquivosDoApp(tmp).includes(`${i18n}/auxiliar/index.ts`))
    assert.ok(falhasEstruturais(tmp).includes(`${i18n}/auxiliar: pasta que não é um idioma declarado`))
    assert.ok(conferirModulos(tmp).includes(`${i18n}/auxiliar: pasta que não é um idioma declarado`))
  } finally { rmSync(tmp, { recursive: true, force: true }) }
})

test('symlink em src falha', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-link-'))
  try {
    mkdirSync(join(tmp, 'src'))
    writeFileSync(join(tmp, 'src/real.ts'), 'export const a = 1\n')
    symlinkSync('real.ts', join(tmp, 'src/atalho.ts'))
    mkdirSync(join(tmp, 'tools'))
    writeFileSync(join(tmp, 'tools/textos-legado.json'), '{ "arquivos": {} }\n')
    writeFileSync(join(tmp, 'tools/javascript-legado.json'), '{ "arquivos": [] }\n')
    assert.deepEqual(falhasEstruturais(tmp), ['src/atalho.ts: symlink em src'])
    assert.deepEqual(conferirArvore(tmp).erros, ['src/atalho.ts: symlink em src'])
  } finally { rmSync(tmp, { recursive: true, force: true }) }
})

test('node_modules dentro de src falha e não é lida', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-nm-'))
  try {
    mkdirSync(join(tmp, 'src/node_modules/pacote'), { recursive: true })
    writeFileSync(join(tmp, 'src/node_modules/pacote/real.ts'), 'export const a = 1\n')
    symlinkSync('real.ts', join(tmp, 'src/node_modules/pacote/atalho'))
    mkdirSync(join(tmp, 'tools'))
    writeFileSync(join(tmp, 'tools/textos-legado.json'), '{ "arquivos": {} }\n')
    writeFileSync(join(tmp, 'tools/javascript-legado.json'), '{ "arquivos": [] }\n')
    assert.deepEqual(falhasEstruturais(tmp), ['src/node_modules: node_modules dentro de src'])
    assert.deepEqual(conferirArvore(tmp).erros, ['src/node_modules: node_modules dentro de src'])
    assert.equal(arquivosDoApp(tmp).some((a) => a.includes('node_modules')), false)
  } finally { rmSync(tmp, { recursive: true, force: true }) }
})

test('.mts e .cts entram na varredura e JavaScript em src falha', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-ext-'))
  try {
    mkdirSync(join(tmp, 'src'))
    writeFileSync(join(tmp, 'src/frase.mts'), "export const x = 'Texto escondido.'\n")
    writeFileSync(join(tmp, 'src/frase.cts'), "export const y = 'Outro texto escondido.'\n")
    for (const ext of ['js', 'jsx', 'mjs', 'cjs']) writeFileSync(join(tmp, `src/solto.${ext}`), 'export const z = 1\n')
    mkdirSync(join(tmp, 'tools'))
    writeFileSync(join(tmp, 'tools/textos-legado.json'), '{ "arquivos": {} }\n')
    writeFileSync(join(tmp, 'tools/javascript-legado.json'), '{ "arquivos": [] }\n')
    const lidos = arquivosDoApp(tmp)
    assert.ok(lidos.includes('src/frase.mts'))
    assert.ok(lidos.includes('src/frase.cts'))
    assert.equal(lidos.some((a) => a.startsWith('src/solto.')), false)
    const r = conferirArvore(tmp)
    assert.ok(r.novos.some((a) => a.arquivo === 'src/frase.mts' && a.texto === 'Texto escondido.'))
    assert.ok(r.novos.some((a) => a.arquivo === 'src/frase.cts' && a.texto === 'Outro texto escondido.'))
    assert.deepEqual(r.erros.sort(), ['cjs', 'js', 'jsx', 'mjs'].map((ext) => `src/solto.${ext}: JavaScript em src`))
  } finally { rmSync(tmp, { recursive: true, force: true }) }
})

test('a árvore do app não acusa o JavaScript que a main já tem nem node_modules', () => {
  const estruturais = falhasEstruturais(raiz)
  assert.equal(estruturais.some((e) => e.endsWith(': JavaScript em src') || e.endsWith(': node_modules dentro de src')), false, estruturais.join('\n'))
  const lidos = arquivosDoApp(raiz)
  assert.ok(lidos.includes('src/main/fork-cloud/personal-runtime-format.mjs'))
  assert.ok(lidos.includes('src/main/fork-cloud/personal-runtime-asar.mjs'))
  assert.equal(lidos.includes('src/renderer/lib/donnabot-avatars/demo/demo.js'), false)
})

test('D506: árvore sem .git (VM da Instabox, snapshot do hook) passa com o JavaScript da lista e falha com o novo', () => {
  const solto = mkdtempSync(join(tmpdir(), 'textos-sem-git-'))
  const escrever = (rel: string, texto: string) => {
    const caminho = join(solto, rel)
    mkdirSync(join(caminho, '..'), { recursive: true })
    writeFileSync(caminho, texto)
  }
  try {
    escrever('src/velho.mjs', 'export const z = 1\n')
    escrever('src/demo/velho.js', 'export const z = 1\n')
    escrever('tools/textos-legado.json', '{ "arquivos": {} }\n')
    escrever('tools/javascript-legado.json', JSON.stringify({ sobre: 'teste', arquivos: ['src/velho.mjs', 'src/demo/velho.js'] }))
    assert.throws(() => execFileSync('git', ['rev-parse', '--git-dir'], { cwd: solto, env: envGitSanitizado(), stdio: 'pipe' }))
    assert.deepEqual(falhasEstruturais(solto), [])
    assert.deepEqual(conferirArvore(solto).erros, [])
    escrever('src/novo.js', 'export const z = 1\n')
    assert.deepEqual(falhasEstruturais(solto), ['src/novo.js: JavaScript em src'])
    assert.deepEqual(naoCresceu(solto, {}).erros, [])
    // A lista não vira licença: entrada sem o arquivo na árvore falha, também sem Git.
    rmSync(join(solto, 'src/novo.js'))
    rmSync(join(solto, 'src/demo/velho.js'))
    assert.deepEqual(falhasEstruturais(solto), ['src/demo/velho.js: sobra em tools/javascript-legado.json; remova a entrada'])
    assert.deepEqual(conferirArvore(solto).erros, ['src/demo/velho.js: sobra em tools/javascript-legado.json; remova a entrada'])
  } finally { rmSync(solto, { recursive: true, force: true }) }
})

test('D506: javascript-legado.json fora do formato ou ausente falha com erro claro, na árvore e na base', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'textos-formato-'))
  const lista = join(tmp, 'tools/javascript-legado.json')
  try {
    mkdirSync(join(tmp, 'src'), { recursive: true })
    mkdirSync(join(tmp, 'tools'))
    writeFileSync(join(tmp, 'src/velho.mjs'), 'export const z = 1\n')
    writeFileSync(join(tmp, 'tools/textos-legado.json'), '{ "arquivos": {} }\n')
    // Ausente: falha (o arquivo existe na main).
    assert.deepEqual(falhasEstruturais(tmp), ['javascript-legado.json inválido: arquivo ausente'])
    const casos: [string, string][] = [
      ['{ "arquivos": [', 'JSON quebrado'],
      ['{ "sobre": "sem arquivos" }', 'falta `arquivos`, um array de caminhos'],
      ['{ "arquivos": "src/velho.mjs" }', 'falta `arquivos`, um array de caminhos'],
      ['[ "src/velho.mjs" ]', 'falta `arquivos`, um array de caminhos'],
      ['{ "arquivos": ["src/velho.mjs", 42] }', '`arquivos` tem item que não é texto'],
    ]
    for (const [conteudo, motivo] of casos) {
      writeFileSync(lista, conteudo)
      // Um erro só, claro: não vira uma lista vazia calada nem um erro por arquivo.
      assert.deepEqual(falhasEstruturais(tmp), [`javascript-legado.json inválido: ${motivo}`], conteudo)
      assert.deepEqual(conferirArvore(tmp).erros, [`javascript-legado.json inválido: ${motivo}`], conteudo)
    }
    writeFileSync(lista, '{ "arquivos": ["src/velho.mjs"] }')
    assert.deepEqual(falhasEstruturais(tmp), [])
  } finally { rmSync(tmp, { recursive: true, force: true }) }

  const c = cenaD506()
  try {
    // Base com a lista quebrada: o PR reprova com o erro claro (e a base sem lista, testada acima, cai no JS do commit).
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/tools/javascript-legado.json', '{ "arquivos": [ 1 ] }\n')
    c.commit('main com a lista fora do formato')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    c.g('merge', '-q', '--ff-only', 'main')
    c.escrever('apps/desktop/tools/javascript-legado.json', '{ "arquivos": [] }\n')
    c.commit('pr corrige a lista')
    const r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => /^base [0-9a-f]+: javascript-legado\.json inválido: `arquivos` tem item que não é texto$/.test(e)), r.erros.join('\n'))
  } finally { c.limpar() }
})

test('scanner: só os módulos registrados ficam de fora; o catálogo plano só na base antiga', () => {
  const i18n = 'src/renderer/lib/fork-i18n'
  assert.equal(foraDoGate(`${i18n}/pt-BR/whatsapp/numero.ts`), true)
  assert.equal(foraDoGate(`${i18n}/en/index.ts`), true)
  for (const c of [`${i18n}/pt-BR/extra.ts`, `${i18n}/index.ts`, `${i18n}/estado.ts`, `${i18n}/registro.ts`, `${i18n}/pt-BR.ts`]) assert.equal(foraDoGate(c), false, c)
  assert.equal(foraDoGate(`${i18n}/pt-BR.ts`, 'base'), true)
  assert.equal(foraDoGate(`${i18n}/pt-BR/extra.ts`, 'base'), false)
})

test('falha com chave faltando, sobrando, vazia ou parâmetro diferente', () => {
  const erros = conferirCatalogos({
    'pt-BR': { a: 'Passo {atual} de {total}', b: 'Oi', c: 'Sim' },
    en: { a: 'Step {atual} of {all}', c: ' ', extra: 'x' },
  }, categorias)
  assert.deepEqual(erros.sort(), [
    'en: a tem parâmetros {all,atual}, pt-BR tem {atual,total}',
    'en: c está vazio',
    'en: chave extra não existe no pt-BR',
    'en: falta a chave b',
  ])
})

test('plural: cada idioma com as categorias do seu locale, sem exigir forma que ele não tem', () => {
  const pt = { 'bots.one': '{n} bot de {dono}', 'bots.many': '{n} de bots de {dono}', 'bots.other': '{n} bots de {dono}' }
  assert.deepEqual(conferirCatalogos({
    'pt-BR': pt,
    'zh-CN': { 'bots.other': '{dono}的 {n} 个机器人' },
    'ar-AE': { 'bots.zero': 'لا بوتات لدى {dono}', 'bots.one': 'بوت واحد لدى {dono}', 'bots.two': 'بوتان لدى {dono}', 'bots.few': '{n} بوتات لدى {dono}', 'bots.many': '{n} بوتًا لدى {dono}', 'bots.other': '{n} بوت لدى {dono}' },
  }, categorias), [])
  const erros = conferirCatalogos({
    'pt-BR': pt,
    en: { 'bots.other': '{n} bots' },
    'zh-CN': { 'bots.one': '一个', 'bots.other': '{n} 个 {dono}' },
    'ar-AE': { 'bots.one': '{dono}', 'bots.other': '{n} {dono}' },
    fr: { 'bots.one': '{n} bot de {dono}', 'bots.other': '{n} bots de {dono}' },
  }, categorias)
  assert.deepEqual(erros.sort(), [
    'ar-AE: plural bots sem a forma few',
    'ar-AE: plural bots sem a forma many',
    'ar-AE: plural bots sem a forma two',
    'ar-AE: plural bots sem a forma zero',
    'en: bots.other tem parâmetros {}, pt-BR tem {dono} (fora {n})',
    'en: plural bots sem a forma one',
    'fr: plural bots sem a forma many',
    'zh-CN: bots.one tem parâmetros {}, pt-BR tem {dono} (fora {n})',
    'zh-CN: plural bots tem a forma one, que o idioma não usa',
  ])
})

test('acha texto visível novo, inclusive de acessibilidade', () => {
  const achados = textos(`
    const opcoes = [{ id: 'claro', label: 'Claro' }]
    export const X = () => <div className="caixa grande" aria-label="Fechar painel" title={ok ? 'Salvo' : 'Erro'}>
      Olá, mundo
      <input placeholder="Buscar bots" />
      <img alt="Foto do bot" />
      {pronto && 'Tudo pronto'}
    </div>
    throw new Error('Não foi possível salvar.')
  `)
  assert.deepEqual(achados.sort(), ['Claro', 'Erro', 'Fechar painel', 'Foto do bot', 'Não foi possível salvar.', 'Olá, mundo', 'Salvo', 'Buscar bots', 'Tudo pronto'].sort())
})

test('ignora identificadores, dados e o que vem dos catálogos', () => {
  const achados = textos(`
    import { x } from 'Algum Modulo Com Espaço'
    type Tema = 'Claro Escuro'
    if (e.key === 'Arrow Down') {}
    switch (v) { case 'Valor Tal': break }
    localStorage.getItem('Chave Com Espaço')
    console.log('Mensagem de log.')
    ipcMain.on('app:Canal Tal', () => {})
    const m = { 'Chave Visível': 1 }
    export const X = () => <p className="Classe Grande" id="Um Id" role="note" lang="pt-BR">{t('idioma.passo.titulo1')}&nbsp;</p>
    const caminho = 'M 585 342 C 543 317'
    const css = 'var(--ink) 0 0 2px'
    const id = 'idioma.passo.continuar'
  `)
  assert.deepEqual(achados, [])
})

test('exceção só pontual de dado, com comentário de verdade e motivo; nunca em texto visível', () => {
  assert.deepEqual(textos(`const a = 'Nome Próprio' // textos-ok: nome de marca\nconst b = 'Outra Frase'`, 'x.ts'), ['Outra Frase'])
  // Sem motivo, ou a palavra numa string comum, não vale.
  assert.deepEqual(textos(`const a = 'Nome Próprio' // textos-ok:`, 'x.ts'), ['Nome Próprio'])
  assert.deepEqual(textos(`const x = "textos-ok: dado"; const C = () => <button>Novo rótulo</button>`), ['Novo rótulo'])
  // Texto JSX, atributo de acessibilidade e prop de texto nunca são dispensados.
  assert.deepEqual(textos(`// textos-ok: tentativa\nconst C = () => <button aria-label="Fechar painel">Novo rótulo</button>`).sort(), ['Fechar painel', 'Novo rótulo'])
  assert.deepEqual(textos(`const o = { label: 'Sistema' } // textos-ok: tentativa`, 'x.ts'), ['Sistema'])
})

test('--listar: por área, com arquivo e linha', () => {
  assert.equal(areaDe('src/renderer/views/fork/sessoes/Lista.tsx'), 'sessoes')
  assert.equal(areaDe('src/renderer/views/fork/abas/Abas.tsx'), 'sessoes')
  assert.equal(areaDe('src/renderer/views/fork/painel-direito/Painel.tsx'), 'sessoes')
  assert.equal(areaDe('src/renderer/views/criar-bot/Passo.tsx'), 'criar-bot')
  assert.equal(areaDe('src/renderer/views/thread/Composer.tsx'), 'conversa')
  assert.equal(areaDe('src/main/tray.ts'), 'main')
  assert.equal(areaDe('src/renderer/lib/qualquer.ts'), 'outros')
  const itens = listar(raiz, 'ajustes')
  assert.ok(itens.length > 0 && itens.every((i) => i.area === 'ajustes' && i.linha > 0))
  // D454: os Ajustes gerais já saíram do legado (fatia 4); a área ainda lista as telas que faltam, com o texto.
  assert.ok(itens.every((i) => i.arquivo.startsWith('src/') && i.texto.length > 0))
  assert.ok(!itens.some((i) => i.arquivo === 'src/renderer/views/settings/SettingsView.tsx'))
  // A base de idiomas não tem texto solto.
  assert.deepEqual(listar(raiz, 'idiomas'), [])
})

test('num PR, o inventário não pode crescer em relação ao código da base (o JSON pode ser editado no patch)', () => {
  const repo = mkdtempSync(join(tmpdir(), 'textos-gate-'))
  const g = (...args: string[]) => execFileSync('git', args, { cwd: repo, env: envGitSanitizado(), stdio: 'ignore' })
  try {
    mkdirSync(join(repo, 'apps/desktop/src'), { recursive: true })
    writeFileSync(join(repo, 'apps/desktop/src/a.tsx'), 'export const A = () => <p>Texto antigo</p>\n')
    // Base anterior à divisão por área: o catálogo plano é o catálogo, não texto legado.
    mkdirSync(join(repo, 'apps/desktop/src/renderer/lib/fork-i18n'), { recursive: true })
    writeFileSync(join(repo, 'apps/desktop/src/renderer/lib/fork-i18n/pt-BR.ts'), "export const ptBR = { 'a.b': 'Texto do catálogo antigo.' } as const\n")
    g('init', '-q'); g('-c', 'user.email=t@t', '-c', 'user.name=t', 'add', '.'); g('-c', 'user.email=t@t', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'base')
    g('update-ref', 'refs/remotes/origin/main', 'HEAD')
    // O patch acrescenta um rótulo e o põe no inventário: a árvore fecha, a base não.
    writeFileSync(join(repo, 'apps/desktop/src/a.tsx'), 'export const A = () => <p aria-label="Novo rótulo">Texto antigo</p>\n')
    // O patch divide o catálogo: no candidato, o caminho antigo só reexporta.
    writeFileSync(join(repo, 'apps/desktop/src/renderer/lib/fork-i18n/pt-BR.ts'), "export { ptBR } from './pt-BR/index.ts'\n")
    const atual = inventariar(join(repo, 'apps/desktop'))
    const base = inventarioDaBase(join(repo, 'apps/desktop'), {}, true)
    assert.ok(!('motivo' in base))
    assert.deepEqual(comparar(atual, base.inventario).novos.map((n) => n.texto), ['Novo rótulo'])
    assert.deepEqual(base.inventario, { 'src/a.tsx': { 'Texto antigo': 1 } })
    assert.deepEqual('motivo' in inventarioDaBase(join(repo, 'apps/desktop'), {}), true)
    // Num PR com a base inalcançável (aqui, sem remoto), reprova; fora do CI, só avisa.
    assert.match(naoCresceu(join(repo, 'apps/desktop'), { GITHUB_BASE_REF: 'main' }).erros[0] ?? '', /^base indisponível num PR/)
    assert.deepEqual(naoCresceu(join(repo, 'apps/desktop'), {}).erros, [])
  } finally {
    rmSync(repo, { recursive: true, force: true })
  }
})

test('Git da fixture e da base ignora todo GIT_* herdado e preserva o repositório externo', () => {
  const externo = mkdtempSync(join(tmpdir(), 'textos-gate-externo-'))
  const g = (...args: string[]) => execFileSync('git', args, { cwd: externo, env: envGitSanitizado(), encoding: 'utf8' }).trim()
  // Inclui HEAD, refs, objetos, índices (normal e alternativo) e arquivos de trabalho, byte a byte.
  const snapshot = () => {
    const arquivos: Record<string, string> = {}
    const andar = (dir: string) => {
      for (const e of readdirSync(join(externo, dir), { withFileTypes: true })) {
        const rel = join(dir, e.name)
        if (e.isDirectory()) andar(rel)
        else arquivos[rel] = readFileSync(join(externo, rel)).toString('base64')
      }
    }
    andar('')
    return { head: g('rev-parse', 'HEAD'), refs: g('show-ref'), arquivos }
  }
  try {
    g('init', '-q')
    // A manutenção automática do Git é assíncrona: num runner ocupado ela pode criar
    // .git/objects/maintenance.lock entre os dois snapshots byte a byte.
    g('config', 'gc.auto', '0')
    g('config', 'maintenance.auto', 'false')
    writeFileSync(join(externo, 'sentinela'), 'Preservar este arquivo.\n')
    g('add', '.')
    g('-c', 'user.email=t@t', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'externo')
    writeFileSync(join(externo, 'indice-alternativo'), readFileSync(join(externo, '.git/index')))
    writeFileSync(join(externo, 'sentinela'), 'Alteração local que não pode ser adicionada.\n')
    const antes = snapshot()
    const contaminado = {
      ...process.env,
      GIT_DIR: join(externo, '.git'), GIT_WORK_TREE: externo,
      GIT_INDEX_FILE: join(externo, 'indice-alternativo'),
      GIT_OBJECT_DIRECTORY: join(externo, 'objetos-inexistentes'),
      GIT_CONFIG_COUNT: 'invalido', GIT_TESTE_FUTURO: 'remover também',
    }
    assert.deepEqual(envGitSanitizado(contaminado), Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_'))))
    // O cenário original inteiro roda em outro processo: não há mutação de process.env compartilhado.
    execFileSync(process.execPath, [
      '--test', '--experimental-strip-types', '--test-name-pattern=^num PR, o inventário não pode crescer',
      fileURLToPath(import.meta.url),
    ], { env: contaminado, stdio: 'pipe' })
    assert.deepEqual(snapshot(), antes, 'o Git do gate não pode alterar o repositório externo')
  } finally {
    rmSync(externo, { recursive: true, force: true })
  }
})

test('o inventário só aceita o que já existia, e sobra também falha', () => {
  const legado = { 'a.tsx': { Sair: 1, Velho: 1 } }
  const atual = { 'a.tsx': { Sair: 2 }, 'b.tsx': { Novo: 1 } }
  const r = comparar(atual, legado)
  assert.deepEqual(r.novos.map((n) => `${n.arquivo}:${n.texto}`), ['a.tsx:Sair', 'b.tsx:Novo'])
  assert.deepEqual(r.sobras, [{ arquivo: 'a.tsx', texto: 'Velho', vezes: 1 }])
})

test('num PR do GitHub, o inventário não cresce em relação à base', (t) => {
  const r = naoCresceu(raiz)
  assert.deepEqual(r.erros, [], 'migre o texto para o catálogo; o inventário só encolhe')
  // Hook e Instabox (snapshot sem .git): vale a conferência da árvore abaixo.
  if (r.aviso) t.skip(r.aviso)
})

/** Repositório mínimo com `origin` local, para o caminho de PR (`GITHUB_BASE_REF`). */
function cenaD506() {
  const repo = mkdtempSync(join(tmpdir(), 'textos-d506-'))
  const bare = mkdtempSync(join(tmpdir(), 'textos-d506-bare-'))
  const run = (cwd: string, args: string[]) => execFileSync('git', args, { cwd, env: envGitSanitizado(), encoding: 'utf8' })
  const g = (...args: string[]) => run(repo, args)
  run(bare, ['init', '--bare', '-q'])
  g('init', '-q')
  g('remote', 'add', 'origin', bare)
  const desktop = join(repo, 'apps/desktop')
  const escrever = (rel: string, texto: string) => {
    const caminho = join(repo, rel)
    mkdirSync(join(caminho, '..'), { recursive: true })
    writeFileSync(caminho, texto)
  }
  const legado = (arquivos: Record<string, Record<string, number>>, sobre = 'teste') =>
    JSON.stringify({ sobre, arquivos }, null, 1) + '\n'
  const commit = (msg: string) => {
    g('add', 'apps')
    g('-c', 'user.email=t@t', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', 'commit', '-qm', msg)
  }
  escrever('apps/desktop/src/a.tsx', 'export const A = () => <p>Texto antigo</p>\n')
  escrever('apps/desktop/tools/textos-legado.json', legado({ 'src/a.tsx': { 'Texto antigo': 1 } }))
  escrever('apps/desktop/tools/javascript-legado.json', JSON.stringify({ sobre: 'teste', arquivos: [] }) + '\n')
  commit('base')
  g('branch', '-M', 'main')
  g('push', '-q', 'origin', 'HEAD:main')
  g('checkout', '-q', '-b', 'pr')
  const envPr = { GITHUB_BASE_REF: 'main', GITHUB_HEAD_REF: 'pr' }
  return {
    repo, bare, desktop, g, escrever, legado, commit, envPr,
    limpar: () => { rmSync(repo, { recursive: true, force: true }); rmSync(bare, { recursive: true, force: true }) },
  }
}

test('base pré-baixada: usa o SHA do evento sem fetch e falha fechado com base inválida ou ausente', () => {
  const c = cenaD506()
  try {
    const base = c.g('rev-parse', 'HEAD').trim()
    c.escrever('apps/desktop/src/novo.tsx', 'export const B = () => <p>Texto novo</p>\n')
    c.commit('pr com texto novo')
    c.g('remote', 'set-url', 'origin', `${c.bare}.indisponivel`)
    const env = { ...c.envPr, MEXERICA_CI_BASE_SHA: base }
    const resultado = inventarioDaBase(c.desktop, env)
    assert.ok(!('motivo' in resultado), JSON.stringify(resultado))
    assert.equal(resultado.ref, base)
    assert.equal(resultado.ponta, base)
    assert.ok(naoCresceu(c.desktop, env).erros.some(e => e.includes('Texto novo')))
    for (const invalida of ['', 'main', '0'.repeat(40), 'f'.repeat(40), `${base}\n`]) {
      const r = inventarioDaBase(c.desktop, { ...env, MEXERICA_CI_BASE_SHA: invalida })
      assert.ok('motivo' in r && r.motivo.startsWith('base indisponível'), invalida)
    }
  } finally { c.limpar() }
})

test('base pré-baixada: merge de PR em clone depth 2 funciona sem credencial nem fetch', () => {
  const c = cenaD506()
  const clone = mkdtempSync(join(tmpdir(), 'textos-ci-base-'))
  try {
    const base = c.g('rev-parse', 'HEAD').trim()
    c.escrever('apps/desktop/src/b.tsx', 'export const B = () => <p>Texto novo</p>\n')
    c.commit('head do PR')
    c.g('checkout', '-q', 'main')
    c.g('-c', 'user.email=t@t', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', 'merge', '--no-ff', '-qm', 'merge do PR', 'pr')
    c.g('push', '-q', 'origin', 'HEAD:ci')
    const g = (...args: string[]) => execFileSync('git', args, { cwd: clone, env: envGitSanitizado(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
    g('clone', '-q', '--depth=2', '--branch', 'ci', pathToFileURL(c.bare).href, '.')
    assert.equal(g('rev-parse', '--is-shallow-repository'), 'true')
    g('remote', 'set-url', 'origin', `${c.bare}.indisponivel`)
    const r = inventarioDaBase(join(clone, 'apps/desktop'), { ...c.envPr, MEXERICA_CI_BASE_SHA: base })
    assert.ok(!('motivo' in r), JSON.stringify(r))
    assert.equal(r.ref, base)
    assert.equal(r.ponta, base)
  } finally { c.limpar(); rmSync(clone, { recursive: true, force: true }) }
})

test('D506: a lista de JavaScript legado não cresce contra o merge-base e pode encolher; JS novo fora da lista falha', () => {
  const c = cenaD506()
  const lista = (arquivos: string[]) => JSON.stringify({ sobre: 'teste', arquivos }) + '\n'
  try {
    // A main já tinha o arquivo e a lista.
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/velho.mjs', 'export const z = 1\n')
    c.escrever('apps/desktop/tools/javascript-legado.json', lista(['src/velho.mjs']))
    c.commit('main com a lista')
    c.g('push', '-q', 'origin', 'HEAD:main')

    // (2) JS novo fora da lista, com Git: falha; o da lista passa.
    c.g('checkout', '-q', 'pr')
    c.g('merge', '-q', '--ff-only', 'main')
    c.escrever('apps/desktop/src/novo.js', 'export const z = 1\n')
    c.commit('pr acrescenta javascript fora da lista')
    let r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => e.includes('src/novo.js: JavaScript em src')), r.erros.join('\n'))
    assert.equal(r.erros.some((e) => e.includes('velho.mjs')), false, r.erros.join('\n'))

    // (3) Pôr o arquivo na lista para passar: a lista cresceu contra o merge-base e reprova.
    c.escrever('apps/desktop/tools/javascript-legado.json', lista(['src/velho.mjs', 'src/novo.js']))
    c.commit('pr põe o arquivo novo na lista')
    r = naoCresceu(c.desktop, c.envPr)
    assert.equal(r.erros.some((e) => e.includes('src/novo.js: JavaScript em src')), false, r.erros.join('\n'))
    assert.ok(r.erros.some((e) => e.includes('javascript-legado.json cresceu') && e.endsWith(': src/novo.js')), r.erros.join('\n'))

    // (4) Encolher: apagar o arquivo novo e a entrada, e também o velho com a sua entrada, passa.
    c.g('rm', '-q', 'apps/desktop/src/novo.js', 'apps/desktop/src/velho.mjs')
    c.escrever('apps/desktop/tools/javascript-legado.json', lista([]))
    c.commit('pr encolhe a lista')
    r = naoCresceu(c.desktop, c.envPr)
    assert.deepEqual(r.erros, [])
  } finally { c.limpar() }
})

test('D506: a primeira versão da lista (a base ainda sem o arquivo) vale para o JavaScript que a base já tinha', () => {
  const c = cenaD506()
  try {
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/velho.mjs', 'export const z = 1\n')
    c.g('rm', '-q', 'apps/desktop/tools/javascript-legado.json')
    c.commit('main já tinha o arquivo, sem lista')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    c.g('merge', '-q', '--ff-only', 'main')
    c.escrever('apps/desktop/tools/javascript-legado.json', JSON.stringify({ sobre: 'teste', arquivos: ['src/velho.mjs'] }) + '\n')
    c.commit('pr cria a lista com o que a base já tinha')
    assert.deepEqual(naoCresceu(c.desktop, c.envPr).erros, [])
    c.escrever('apps/desktop/src/novo.js', 'export const z = 1\n')
    c.escrever('apps/desktop/tools/javascript-legado.json', JSON.stringify({ sobre: 'teste', arquivos: ['src/velho.mjs', 'src/novo.js'] }) + '\n')
    c.commit('pr inclui um arquivo novo na lista')
    assert.ok(naoCresceu(c.desktop, c.envPr).erros.some((e) => e.includes('javascript-legado.json cresceu')))
  } finally { c.limpar() }
})

test('PR desatualizado depois que a main migra textos passa', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/n.ts', 'export const N = 1\n')
    c.commit('pr mexe em outro arquivo')
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/a.tsx', "export const A = () => <p>{t('a')}</p>\n")
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({}))
    c.commit('main migra')
    c.g('push', '-q', 'origin', 'HEAD:main')
    // A ponta da main já não tem o texto. Comparar o inventário do PR com ela acusaria crescimento falso.
    const fonteDaPonta = inventariar(c.desktop, 'base')
    c.g('checkout', '-q', 'pr')
    assert.ok(comparar(lerInventario(c.desktop), fonteDaPonta).novos.some((n) => n.texto === 'Texto antigo'))
    const r = naoCresceu(c.desktop, c.envPr)
    assert.deepEqual(r.erros, [], r.erros.join('\n'))
    assert.deepEqual(r.avisos, [])
    assert.deepEqual(naoCresceu(c.desktop, {}, true).erros, [])
  } finally { c.limpar() }
})

test('PR que acrescenta um texto solto falha', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/a.tsx', 'export const A = () => <p>Texto antigo</p>\nexport const B = () => <p>Novo rótulo</p>\n')
    c.commit('pr acrescenta texto')
    const r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => /texto solto/.test(e) && e.includes('Novo rótulo')), r.erros.join('\n'))
  } finally { c.limpar() }
})

test('entrada nova no inventário falha contra o merge-base', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/a.tsx', 'export const A = () => <p>Texto antigo</p>\nexport const B = () => <p>Novo rótulo</p>\n')
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({ 'src/a.tsx': { 'Texto antigo': 1, 'Novo rótulo': 1 } }))
    c.commit('pr inventaria o texto novo')
    const r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => /inventário cresceu/.test(e) && e.includes('Novo rótulo')), r.erros.join('\n'))
    assert.ok(!r.erros.some((e) => /texto solto|sobra/.test(e)), r.erros.join('\n'))
  } finally { c.limpar() }
})

test('a junção que reintroduz na main um texto já migrado falha na simulação', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/a.tsx', 'export const A = () => <section><p>Texto antigo</p></section>\n')
    c.commit('pr reescreve o arquivo')
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/a.tsx', "export const A = () => <p>{t('a')}</p>\n")
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({}))
    c.commit('main migra')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    const noPr = conferirArvore(c.desktop)
    assert.deepEqual(noPr.novos, [])
    assert.deepEqual(noPr.sobras, [])
    const r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => /texto solto/.test(e) && e.includes('Texto antigo')), r.erros.join('\n'))
    assert.ok(!r.erros.some((e) => /inventário cresceu/.test(e)), r.erros.join('\n'))
  } finally { c.limpar() }
})

test('sobra criada pelo próprio PR falha', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/a.tsx', "export const A = () => <p>{t('a')}</p>\n")
    c.commit('pr tira o texto e não encolhe')
    const r = naoCresceu(c.desktop, c.envPr)
    assert.ok(r.erros.some((e) => /sobra do próprio PR/.test(e) && e.includes('Texto antigo')), r.erros.join('\n'))
  } finally { c.limpar() }
})

test('sobra vinda da main só avisa', () => {
  const c = cenaD506()
  try {
    c.escrever('apps/desktop/src/n.ts', 'export const N = 1\n')
    c.commit('pr mexe em outro arquivo')
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/a.tsx', "export const A = () => <p>{t('a')}</p>\n")
    c.commit('main tira o texto e deixa o inventário')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    assert.deepEqual(conferirArvore(c.desktop).sobras, [])
    const r = naoCresceu(c.desktop, c.envPr)
    assert.deepEqual(r.erros, [], r.erros.join('\n'))
    assert.match(r.avisos.join('\n'), /sobra vinda da main/)
    assert.match(r.avisos.join('\n'), /Texto antigo/)
  } finally { c.limpar() }
})

test('conflito em textos-legado.json não falha o gate', () => {
  const c = cenaD506()
  try {
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({ 'src/a.tsx': { 'Texto antigo': 1 } }, 'main'))
    c.commit('main muda o cabeçalho')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({ 'src/a.tsx': { 'Texto antigo': 1 } }, 'pr'))
    c.commit('pr muda o cabeçalho')
    const r = naoCresceu(c.desktop, c.envPr)
    assert.deepEqual(r.erros, [], r.erros.join('\n'))
    assert.match(r.avisos.join('\n'), /conflita na junção/)
  } finally { c.limpar() }
})

test('módulo que só a main registrou não vira texto solto na junção', () => {
  const c = cenaD506()
  try {
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/renderer/lib/fork-i18n/registro.ts', [
      "export const IDIOMAS_CATALOGO = ['pt-BR'] as const",
      'export const MODULOS = [',
      "  { modulo: 'soNaMain', prefixo: 'so.' },",
      ']',
      '',
    ].join('\n'))
    c.escrever('apps/desktop/src/renderer/lib/fork-i18n/pt-BR/soNaMain.ts', "export const textos = { 'so.frase': 'Frase que só a main registrou.' }\n")
    c.commit('main registra um módulo')
    c.g('push', '-q', 'origin', 'HEAD:main')
    c.g('checkout', '-q', 'pr')
    assert.ok(textosVisiveis('x.ts', "export const textos = { 'so.frase': 'Frase que só a main registrou.' }\n").some((a) => a.texto.startsWith('Frase que só')))
    const r = naoCresceu(c.desktop, c.envPr)
    assert.deepEqual(r.erros, [], r.erros.join('\n'))
  } finally { c.limpar() }
})

test('clone raso de PR desatualizado ainda passa', () => {
  const c = cenaD506()
  const raso = mkdtempSync(join(tmpdir(), 'textos-d506-raso-'))
  try {
    c.escrever('apps/desktop/src/n.ts', 'export const N = 1\n')
    c.commit('pr mexe em outro arquivo')
    c.g('push', '-q', 'origin', 'HEAD:pr')
    c.g('checkout', '-q', 'main')
    c.escrever('apps/desktop/src/a.tsx', "export const A = () => <p>{t('a')}</p>\n")
    c.escrever('apps/desktop/tools/textos-legado.json', c.legado({}))
    c.commit('main migra')
    c.g('push', '-q', 'origin', 'HEAD:main')
    execFileSync('git', ['clone', '--depth', '1', '-b', 'pr', `file://${c.bare}`, raso], { env: envGitSanitizado() })
    const r = naoCresceu(join(raso, 'apps/desktop'), c.envPr)
    assert.deepEqual(r.erros, [], r.erros.join('\n'))
  } finally {
    c.limpar()
    rmSync(raso, { recursive: true, force: true })
  }
})

test('a árvore do app não tem texto solto novo nem sobra no inventário', () => {
  const r = conferirArvore(raiz)
  const msg = [...r.novos.map((a) => `texto solto: ${a.arquivo}:${a.linha} ${JSON.stringify(a.texto)} (use o catálogo em src/renderer/lib/fork-i18n)`),
    ...r.sobras.map((s) => `sobra: ${s.arquivo} ${JSON.stringify(s.texto)} ×${s.vezes} (rode node --experimental-strip-types tools/textos-gate.ts --encolher)`)].join('\n')
  assert.equal(msg, '')
})
