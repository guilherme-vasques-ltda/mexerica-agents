// R1 do #588 (D454, checklist de bidi): idioma nas raízes efetivas sem nova caixa de layout, nomes com a direção deles
// e horários sempre LTR. Fonte, não render: as folhas usam portal e o Icon não renderiza no servidor.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { textosEm } from './textos.ts'

const fonte = (arquivo: string) => readFileSync(fileURLToPath(new URL(`./${arquivo}`, import.meta.url)), 'utf8')
const SUPERFICIE = /<div className="idioma-superficie" style=\{\{ display: 'contents' \}\} \{\.\.\.superficieIdioma\(idioma\)\}>/

test('R1-01/02: cada raiz efetiva (folha em portal, painel irmão do terminal, motores) declara o idioma sem criar caixa', () => {
  for (const arquivo of ['ConectarConta.tsx', 'ContasDoHub.tsx', 'NovaConta.tsx', 'ChaveDeApi.tsx', 'TerminalDeSetup.tsx', 'MotoresDoHub.tsx', 'RemoteEngineSheet.tsx']) {
    const codigo = fonte(arquivo)
    assert.match(codigo, SUPERFICIE, `${arquivo}: superfície de idioma em display: contents`)
    assert.doesNotMatch(codigo, /<div className="idioma-superficie" \{\.\.\.superficieIdioma/, `${arquivo}: sem caixa nova que quebre a coluna flex`)
  }
  // O conteúdo técnico do terminal fica LTR em qualquer idioma.
  assert.match(fonte('TerminalDeSetup.tsx'), /className="setup-terminal" dir="ltr"/)
})

test('R1-03: nomes de conta, bot e motor com a direção do próprio texto; isolados nos rótulos', () => {
  const lista = fonte('ContasDoHub.tsx')
  assert.match(lista, /truncate dir="auto" title=\{isolado\(view\.title\)\}>\{view\.title\}/)
  assert.match(lista, /title=\{<bdi dir="auto">\{view\.title\}<\/bdi>\}/)
  assert.match(lista, /tone="text" dir="auto">\{bot\.name\}/)
  assert.match(lista, /aria-label=\{`\$\{isolado\(view\.title\)\}/)
  const motores = fonte('MotoresDoHub.tsx')
  assert.match(motores, /dir="auto" title=\{isolado\(b\.name\)\}>\{b\.name\}/)
  assert.match(motores, /aria-label=\{`\$\{isolado\(b\.name\)\}/)
})

test('R1-04: horários e porcentagem no locale, mas sempre LTR (LRI…PDI), também em árabe', () => {
  const frase = textosEm('ar-AE').valeAte('02:32 م')
  assert.ok(frase.includes('⁦02:32 م⁩'), frase)
  assert.match(fonte('ContasDoHub.tsx'), /quando: ltr\(whenText\(w\.resetsAt, idioma\)\)/)
  assert.match(fonte('ContasDoHub.tsx'), /const pct = formatarNumero\(idioma, Math\.round\(percent\) \/ 100, \{ style: 'percent' \}\)/)
  assert.match(fonte('ContasDoHub.tsx'), /pct: ltr\(pct\)/)
})
