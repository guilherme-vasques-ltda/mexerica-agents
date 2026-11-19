// D454 (fatia 10a): Sheet e FormSheet aceitam `language` e o põem na raiz do portal (lang/dir), como Dialog e AnchoredMenu;
// sem a prop, a raiz continua como antes. Estático (o portal vai para document.body e o Node não tem DOM aqui).
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import ts from 'typescript'

const ler = (rel: string) => {
  const arquivo = new URL(rel, import.meta.url)
  return ts.createSourceFile(arquivo.pathname, readFileSync(arquivo, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
}
const funcao = (sf: ts.SourceFile, nome: string) => sf.statements.find((s): s is ts.FunctionDeclaration => ts.isFunctionDeclaration(s) && s.name?.text === nome)!

test('Sheet: a raiz do portal recebe lang/dir só quando há language; sem ele, a classe é a de sempre', () => {
  const sf = ler('../components/Overlay.tsx')
  const sheet = funcao(sf, 'Sheet')
  const params = sheet.parameters[0]!.name.getText(sf)
  assert.match(params, /\blanguage\b/)
  assert.match(sheet.parameters[0]!.type!.getText(sf), /language\?: PortalLanguage/)
  let raiz: ts.JsxOpeningElement | ts.JsxSelfClosingElement | null = null
  const ver = (n: ts.Node): void => {
    if ((ts.isJsxOpeningElement(n) || ts.isJsxSelfClosingElement(n)) && n.attributes.properties.some((a) => ts.isJsxAttribute(a) && a.name.getText(sf) === 'role' && a.initializer?.getText(sf) === '"dialog"')) raiz = n
    ts.forEachChild(n, ver)
  }
  ver(sheet)
  assert.ok(raiz, 'raiz role="dialog" do Sheet')
  const props = (raiz as ts.JsxOpeningElement).attributes.properties
  // lang/dir por spread de `language`.
  assert.ok(props.some((a) => ts.isJsxSpreadAttribute(a) && a.expression.getText(sf) === 'language'))
  // Sem language, a classe é só `modal-card ${shown ? 'shown' : ''}`; com ele, acrescenta idioma-superficie.
  const classe = props.find((a): a is ts.JsxAttribute => ts.isJsxAttribute(a) && a.name.getText(sf) === 'className')!.initializer!.getText(sf)
  assert.equal(classe, "{`modal-card ${shown ? 'shown' : ''}${language ? ' idioma-superficie' : ''}`}")
})

test('FormSheet repassa language ao Sheet', () => {
  const sf = ler('../views/marketplace/pieces.tsx')
  const form = funcao(sf, 'FormSheet')
  assert.match(form.parameters[0]!.type!.getText(sf), /language\?: PortalLanguage/)
  assert.match(form.body!.getText(sf), /<Sheet [^>]*language=\{language\}/)
})
