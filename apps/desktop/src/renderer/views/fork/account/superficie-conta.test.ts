// R1-02 e R1-03 do #596 (D70): estático, lê o código (a tela depende do app, da ponte e de efeitos; o SSR não chega lá).
// R1-02: o título e o rodapé do CardSection da Conta ficam dentro da superfície de idioma.
// R1-03: o campo de código da reconfirmação (one-time-code) é da esquerda para a direita.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import ts from 'typescript'

const ler = (nome: string) => {
  const arquivo = new URL(`./${nome}`, import.meta.url)
  return ts.createSourceFile(nome, readFileSync(arquivo, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
}
const tag = (n: ts.JsxElement | ts.JsxSelfClosingElement) => (ts.isJsxElement(n) ? n.openingElement : n).tagName.getText()
const atributos = (n: ts.JsxElement | ts.JsxSelfClosingElement) => (ts.isJsxElement(n) ? n.openingElement : n).attributes.properties
function elementos(sf: ts.SourceFile) {
  const out: (ts.JsxElement | ts.JsxSelfClosingElement)[] = []
  const visit = (n: ts.Node) => { if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n)) out.push(n); ts.forEachChild(n, visit) }
  visit(sf)
  return out
}
const temSuperficie = (n: ts.JsxElement | ts.JsxSelfClosingElement) =>
  atributos(n).some((a) => ts.isJsxSpreadAttribute(a) && /superficieIdioma\(/.test(a.expression.getText()))

test('R1-02: o CardSection da Conta (título, linhas e rodapé) está dentro de um elemento com superficieIdioma', () => {
  const sf = ler('AccountSettingsCard.tsx')
  const secoes = elementos(sf).filter((e) => tag(e) === 'CardSection')
  assert.ok(secoes.length > 0)
  for (const secao of secoes) {
    let pai: ts.Node | undefined = secao.parent
    while (pai && !(ts.isJsxElement(pai) && temSuperficie(pai))) pai = pai.parent
    assert.ok(pai, `CardSection em AccountSettingsCard.tsx:${sf.getLineAndCharacterOfPosition(secao.getStart()).line + 1} fora da superfície`)
  }
})

test('R1-03: todo campo de código (autoComplete one-time-code) do perfil declara dir="ltr" para código', () => {
  const sf = ler('AccountProfile.tsx')
  const campos = elementos(sf).filter((e) => tag(e) === 'input' && atributos(e).some((a) => ts.isJsxAttribute(a) && a.name.getText() === 'autoComplete' && /one-time-code/.test(a.initializer?.getText() ?? '')))
  assert.ok(campos.length > 0)
  for (const campo of campos) {
    const dir = atributos(campo).find((a) => ts.isJsxAttribute(a) && a.name.getText() === 'dir')
    assert.ok(dir && /'ltr'|"ltr"/.test(dir.getText()), `input em AccountProfile.tsx:${sf.getLineAndCharacterOfPosition(campo.getStart()).line + 1} sem dir="ltr"`)
  }
})
