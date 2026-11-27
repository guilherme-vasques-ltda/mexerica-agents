// Biblioteca de ícones da Mexerica (brand/assets/icon/mexerica): o pacote bate com HASHES.tsv, todo ícone que o app usa
// vem do manifesto e todo botão que passou a desenhar um ícone dele continua com rótulo acessível.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { montarIcones, mioloDoSvg, type Manifesto } from '../components/fork-icones-mexerica-mapa.ts'

const pasta = fileURLToPath(new URL('../../../../../brand/assets/icon/mexerica/', import.meta.url))
const renderer = fileURLToPath(new URL('..', import.meta.url))
const manifesto = JSON.parse(readFileSync(join(pasta, 'manifesto.json'), 'utf8')) as Manifesto & { total: number; triadeDoCompositor: string[] }
const arquivos = new Map(manifesto.icones.map((i) => [i.arquivo, readFileSync(join(pasta, i.arquivo), 'utf8')]))
const icones = montarIcones(manifesto, arquivos)
const ids = new Set(manifesto.icones.map((i) => i.id))

test('o pacote em brand/ bate com o HASHES.tsv e o manifesto', () => {
  const linhas = readFileSync(join(pasta, 'HASHES.tsv'), 'utf8').trim().split('\n').slice(1).map((l) => l.split('\t'))
  for (const [caminho, sha, bytes] of linhas) {
    const dados = readFileSync(join(pasta, caminho))
    assert.equal(createHash('sha256').update(dados).digest('hex'), sha, caminho)
    assert.equal(dados.length, Number(bytes), caminho)
  }
  const listados = new Set(linhas.map(([caminho]) => caminho))
  assert.equal(manifesto.icones.length, manifesto.total)
  assert.equal(ids.size, manifesto.total)
  for (const i of manifesto.icones) assert.ok(listados.has(i.arquivo), i.arquivo)
  assert.deepEqual(readdirSync(join(pasta, 'svg')).map((f) => `svg/${f}`).sort(), manifesto.icones.map((i) => i.arquivo).sort())
})

test('todo ícone do manifesto vira um desenho de traço em currentColor, e o símbolo leva ao mesmo desenho', () => {
  for (const i of manifesto.icones) {
    assert.ok(icones.has(i.id), i.id)
    for (const s of i.simbolosDoApp) assert.equal(icones.get(s), icones.get(i.id), s)
  }
  assert.equal(icones.get('mic'), icones.get('mx-gravar-audio'))
  assert.equal(icones.has('chevron.nao.existe'), false)
  assert.equal(mioloDoSvg('<svg viewBox="0 0 24 24"><path d="M0 0"/></svg>'), null)
  assert.equal(mioloDoSvg(arquivos.get('svg/mx-enviar.svg')!.replace('<path', '<script>x</script><path')), null)
  const [enviar, gravar, vivo] = manifesto.triadeDoCompositor.map((id) => icones.get(id))
  assert.deepEqual(manifesto.triadeDoCompositor, ['mx-enviar', 'mx-gravar-audio', 'mx-voz-ao-vivo'])
  assert.equal(new Set([enviar, gravar, vivo]).size, 3)
})

function tsxDoRenderer(dir: string): string[] {
  return readdirSync(dir).flatMap((nome) => {
    const caminho = join(dir, nome)
    if (statSync(caminho).isDirectory()) return tsxDoRenderer(caminho)
    return caminho.endsWith('.tsx') && !caminho.endsWith('.test.tsx') ? [caminho] : []
  })
}

const atributo = (el: ts.JsxOpeningLikeElement, nome: string) =>
  el.attributes.properties.find((p): p is ts.JsxAttribute => ts.isJsxAttribute(p) && p.name.getText() === nome)

function literais(no: ts.Node | undefined): string[] {
  if (!no) return []
  if (ts.isStringLiteralLike(no)) return [no.text]
  return no.getChildren().flatMap(literais)
}

/** O botão em volta do ícone (até dois níveis de `span`), ou null quando o ícone não está num botão. */
function botaoEmVolta(no: ts.Node): ts.JsxElement | null {
  let atual: ts.Node | undefined = no.parent
  for (let nivel = 0; atual && nivel < 6; atual = atual.parent, nivel++) {
    if (ts.isJsxElement(atual)) {
      const tag = atual.openingElement.tagName.getText()
      if (tag === 'button') return atual
      if (tag !== 'span') return null
    }
  }
  return null
}

const temTextoProprio = (botao: ts.JsxElement): boolean =>
  botao.children.some((filho) => {
    if (ts.isJsxText(filho)) return filho.text.trim() !== ''
    if (ts.isJsxExpression(filho)) return true
    if (ts.isJsxSelfClosingElement(filho)) return filho.tagName.getText() !== 'Icon'
    return ts.isJsxElement(filho) && filho.openingElement.tagName.getText() !== 'Icon' && (temTextoProprio(filho) || filho.children.length > 0)
  })

test('todo ícone mx- do app está no manifesto e todo botão com ícone da biblioteca mantém o rótulo acessível', () => {
  const semRotulo: string[] = []
  let botoes = 0
  for (const caminho of tsxDoRenderer(renderer)) {
    const fonte = ts.createSourceFile(caminho, readFileSync(caminho, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
    const visitar = (no: ts.Node): void => {
      if ((ts.isJsxSelfClosingElement(no) || ts.isJsxOpeningElement(no)) && no.tagName.getText() === 'Icon') {
        const nomes = literais(atributo(no, 'name')?.initializer)
        for (const n of nomes.filter((n) => n.startsWith('mx-'))) assert.ok(ids.has(n), `${relative(renderer, caminho)}: ${n} fora do manifesto`)
        const botao = nomes.some((n) => icones.has(n)) ? botaoEmVolta(no) : null
        if (botao && !atributo(botao.openingElement, 'aria-hidden')) {
          botoes++
          const rotulo = atributo(botao.openingElement, 'aria-label') ?? atributo(botao.openingElement, 'title')
          if (!rotulo && !temTextoProprio(botao)) semRotulo.push(`${relative(renderer, caminho)}:${fonte.getLineAndCharacterOfPosition(no.getStart()).line + 1}`)
        }
      }
      ts.forEachChild(no, visitar)
    }
    visitar(fonte)
  }
  assert.ok(botoes > 20, `só ${botoes} botões com ícone da biblioteca`)
  assert.deepEqual(semRotulo, [])
})

test('compositor: enviar, gravar áudio e ao vivo são três botões, cada um com o próprio rótulo e o próprio ícone', () => {
  const fonte = readFileSync(join(renderer, 'views/thread/Composer.tsx'), 'utf8')
  assert.match(fonte, /send: \['mx-enviar', t\('conversa\.compositor\.enviar'\)\]/)
  assert.match(fonte, /\(\['stop\.fill', 'mx-enviar'\] as const\)/)
  assert.match(fonte, /aria-label=\{label\}\s+title=\{label\}/)
  assert.match(fonte, /aria-label=\{t\(dictation\.listening \? 'conversa\.compositor\.pararDitado' : 'conversa\.compositor\.ditar'\)\}[\s\S]{0,400}<Icon name="mx-gravar-audio"/)
  assert.match(fonte, /\{onCall && !editando \? \(\s*<button[^>]*aria-label=\{t\('conversa\.compositor\.aoVivo'\)\} title=\{t\('conversa\.compositor\.aoVivo'\)\}[^>]*>\s*<Icon name="mx-voz-ao-vivo"/)
  assert.doesNotMatch(fonte, /name="(arrow\.up|waveform|mx-seta-cima|mx-ondas-de-voz|mx-reproduzir)"/)
})
