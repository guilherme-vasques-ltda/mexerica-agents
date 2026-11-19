// D454: componentes compartilhados com padrão fixo em pt-BR não podem aparecer em tela já migrada
// para o catálogo de idiomas sem o prop traduzido. Estático e barato: lê o código, não renderiza.
//
// "Arquivo migrado" = .tsx de src/renderer que importa useT/tradutor/traduzir/useIdioma/superficieIdioma de lib/fork-i18n
// E não está em tools/textos-legado.json (o inventário do legado, que ainda tem texto solto).
// Regras, só em arquivo migrado:
//   <ModalHeader   → closeTitle=   (padrão 'Fechar')
//   <Dialog        → cancel=       (padrão 'Cancelar'; `cancel={null}` também vale: não mostra o botão)
//   <Dialog, <AnchoredMenu, <DropdownMenu, <ChoicePicker, <Sheet, <FormSheet → language=
// Um `{...props}` no elemento conta como quem fornece o prop (não dá para provar o conteúdo, mas o
// tipo do componente já exige o formato).
import assert from 'node:assert/strict'
import test from 'node:test'
import ts from 'typescript'
import { arquivosMigrados, importaCatalogo } from './superficies-fonte.ts'

// Exige cada prop (nome → motivo) por tag.
const regras: Record<string, Record<string, string>> = {
  ModalHeader: { closeTitle: "o padrão é 'Fechar'" },
  Dialog: { cancel: "o padrão é 'Cancelar'", language: 'o diálogo abre no portal, fora do idioma da tela' },
  AnchoredMenu: { language: 'o menu abre no portal, fora do idioma da tela' },
  DropdownMenu: { language: 'o menu abre no portal, fora do idioma da tela' },
  ChoicePicker: { language: 'o menu abre no portal, fora do idioma da tela' },
  Sheet: { language: 'a folha abre no portal, fora do idioma da tela' },
  FormSheet: { language: 'a folha abre no portal, fora do idioma da tela' },
}

// Exceções, no formato "caminho relativo a apps/desktop|Tag|prop". Os próprios componentes (Overlay.tsx,
// Controls.tsx) só repassam `language` ou nem importam o catálogo; uma exceção temporária cita o PR que a tira.
const permitidos = new Set<string>([])

function violacoes(): { falhas: string[]; migrados: number } {
  const falhas: string[] = []
  const lista = arquivosMigrados()
  const migrados = lista.length
  for (const { rel, sf } of lista) {
    const visitar = (node: ts.Node): void => {
      if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
        const tag = node.tagName.getText(sf)
        const exigidos = regras[tag]
        if (exigidos) {
          const props = node.attributes.properties
          const temSpread = props.some((p) => ts.isJsxSpreadAttribute(p))
          const nomes = new Set(props.filter(ts.isJsxAttribute).map((p) => p.name.getText(sf)))
          for (const [prop, motivo] of Object.entries(exigidos)) {
            if (nomes.has(prop) || temSpread || permitidos.has(`${rel}|${tag}|${prop}`)) continue
            const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf))
            falhas.push(`${rel}:${line + 1}: <${tag}> sem ${prop}= (${motivo}); passe o texto/idioma traduzido`)
          }
        }
      }
      ts.forEachChild(node, visitar)
    }
    visitar(sf)
  }
  return { falhas, migrados }
}

test('telas migradas passam closeTitle, cancel e language aos componentes compartilhados (D454)', () => {
  const { falhas, migrados } = violacoes()
  assert.ok(migrados > 50, `só ${migrados} arquivos migrados achados: o critério de "migrado" quebrou`)
  assert.deepEqual(falhas, [], `\n${falhas.join('\n')}\n`)
})

test('o critério pega arquivo migrado com o padrão pt-BR e deixa o legado de fora', () => {
  const sf = (codigo: string) => ts.createSourceFile('x.tsx', codigo, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  assert.equal(importaCatalogo(sf("import { useT } from '../../lib/fork-i18n/estado'")), true)
  assert.equal(importaCatalogo(sf("import { formatarNumero } from '../lib/fork-i18n/index'")), false)
  assert.equal(importaCatalogo(sf("import { useT } from './outra'")), false)
})
