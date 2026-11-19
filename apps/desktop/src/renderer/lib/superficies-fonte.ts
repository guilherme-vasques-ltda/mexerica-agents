// Ajuda das guardas estáticas de idioma (superficies-migradas.test.ts, superficies-direcao.test.ts):
// acha os .tsx "migrados" de src/renderer. Migrado = importa useT/tradutor/traduzir/useIdioma/superficieIdioma
// de lib/fork-i18n E não está em tools/textos-legado.json (o inventário do legado, que ainda tem texto solto).
import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const desktop = fileURLToPath(new URL('../../../', import.meta.url))
const renderer = join(desktop, 'src', 'renderer')

export const importaCatalogo = (sf: ts.SourceFile): boolean =>
  sf.statements.some((s) =>
    ts.isImportDeclaration(s)
    && ts.isStringLiteral(s.moduleSpecifier)
    && /fork-i18n/.test(s.moduleSpecifier.text)
    && !!s.importClause?.namedBindings
    && ts.isNamedImports(s.importClause.namedBindings)
    && s.importClause.namedBindings.elements.some((e) => ['useT', 'tradutor', 'traduzir', 'useIdioma', 'superficieIdioma'].includes((e.propertyName ?? e.name).text)))

function arquivosTsx(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name)
    if (e.isDirectory()) return e.name === 'node_modules' ? [] : arquivosTsx(p)
    return e.name.endsWith('.tsx') ? [p] : []
  })
}

export const legado = new Set(Object.keys(JSON.parse(readFileSync(join(desktop, 'tools', 'textos-legado.json'), 'utf8')).arquivos))

/** Os .tsx migrados, já com o parser do TypeScript. `rel` é o caminho relativo a apps/desktop. */
export function arquivosMigrados(): { rel: string; sf: ts.SourceFile }[] {
  const saida: { rel: string; sf: ts.SourceFile }[] = []
  for (const arquivo of arquivosTsx(renderer)) {
    const rel = relative(desktop, arquivo).split(sep).join('/')
    if (legado.has(rel)) continue
    const sf = ts.createSourceFile(arquivo, readFileSync(arquivo, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
    if (importaCatalogo(sf)) saida.push({ rel, sf })
  }
  return saida
}

export const linhaDe = (sf: ts.SourceFile, node: ts.Node): number => sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1
