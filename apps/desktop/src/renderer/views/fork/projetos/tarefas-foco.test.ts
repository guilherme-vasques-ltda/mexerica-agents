import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

// Revisão de UI do #722 (P2-1/P2-2/P2-3): sem DOM nos testes do desktop, a ligação das telas às regras puras (testadas em
// fork-tarefas.test.ts e tarefas-session.test.ts) é conferida no fonte, como em conversa/arquivar.test.ts.
const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
const composer = (src: string) => src.slice(src.indexOf('function Composer('))

test('P2-3: campo de comentário bloqueado fica readOnly + aria-disabled, nunca disabled (o foco fica no Cmd/Ctrl+Enter)', () => {
  const c = composer(fonte('./TarefaDetalhe.tsx'))
  const area = c.slice(c.indexOf('<textarea'), c.indexOf('/>', c.indexOf('<textarea')))
  assert.doesNotMatch(area, /(?<![-\w])disabled=/, 'disabled tira o foco a cada envio e a cada gravação')
  assert.match(area, /readOnly=\{blocked \|\| sending\}/)
  assert.match(area, /aria-disabled=\{blocked \|\| undefined\}/)
  const send = c.slice(c.indexOf('className="tf-send"'), c.indexOf('</button>', c.indexOf('className="tf-send"')))
  assert.doesNotMatch(send, /(?<![-\w])disabled=/, 'o botão Enviar também não perde o foco ao esvaziar o campo')
  assert.match(send, /aria-disabled=/)
  assert.match(c, /if \(!body \|\| blocked \|\| sending \|\| /, 'sem disabled, o envio continua barrado no submit')
  assert.match(fonte('./tarefas.css'), /\.tf-send\[aria-disabled="true"\]/)
})

test('P2-3: cada tarefa aberta leva o foco ao título; voltar devolve à linha/cartão da tarefa', () => {
  const d = fonte('./TarefaDetalhe.tsx')
  assert.match(d, /useEffect\(\(\) => \{ title\.current\?\.focus\(\) \}, \[\]\)/, 'foco no título a cada montagem (key por tarefa)')
  assert.match(d, /<h2 ref=\{title\} tabIndex=\{-1\} className="tf-detail-title">/)
  assert.match(d, /<h2 ref=\{title\} tabIndex=\{-1\}>\{t\('projetos\.tarefas\.sumiu'\)\}/, 'tarefa sumida também')
  const v = fonte('./TarefasView.tsx')
  assert.match(v, /key=\{`\$\{opened\.projectId\}:\$\{opened\.taskId\}`\}/, 'detalhe remonta a cada tarefa (pai/subtarefa)')
  assert.doesNotMatch(v, /\[!!opened\]/, 'o foco não vai mais ao Voltar a cada abertura')
  assert.match(v, /className="tf-row-open" data-open=\{`\$\{e\.project\.id\}:\$\{e\.task\.id\}`\}/, 'linha da Lista')
  assert.match(v, /className="pk-card-open" data-task=\{e\.task\.id\} data-open=\{`\$\{e\.project\.id\}:\$\{e\.task\.id\}`\}/, 'cartão do Quadro')
  assert.match(v, /\[openKey\(was\), fromRow\.current\]/, 'volta à tarefa que estava aberta, senão à de onde o detalhe abriu')
  assert.match(v, /\(row \?\? back\.current\)\?\.focus\(\)/)
})

test('P2-1 e P2-2: as telas usam as regras puras', () => {
  const sheet = fonte('./TarefaSheet.tsx')
  assert.match(sheet, /syncSheetMeta\(baseMeta, meta, phasesText, nowMeta\)/, 'a releitura compara as fases digitadas')
  assert.doesNotMatch(sheet, /editState\(baseMeta/, 'nada de follow que reescreve o texto das fases')
  assert.equal(sheet.match(/fillMeta\(nowMeta\)/g)?.length, 1, 'reescrever o texto das fases com a versão do Hub só no “Usar a versão atual”')
  assert.match(sheet, /const m = sheetMeta\(meta, phasesText\)/)
  const c = composer(fonte('./TarefaDetalhe.tsx'))
  assert.match(c, /pendingDraft\(pending\.text, text, session\.commentConfirmed\(pending\.id\), open\)/)
  assert.match(c, /r\.status === 'uncertain' && r\.requestId\) setPending\(\{ id: r\.requestId, text: body \}\)/)
})

test('P3: abas da Atividade no padrão completo (nome, painel, uma parada de Tab, setas na direção visual)', () => {
  const d = fonte('./TarefaDetalhe.tsx')
  const tabs = d.slice(d.indexOf('function ActivityTabs('), d.indexOf('/** O texto do comentário'))
  assert.match(tabs, /role="tablist" aria-label=\{t\('projetos\.tarefas\.atividadeMostrar'\)\}/)
  assert.match(tabs, /role="tab" aria-selected=\{tab === id\} aria-controls="tf-activity-panel" tabIndex=\{tab === id \? 0 : -1\}/)
  assert.match(tabs, /getComputedStyle\(list\.current\)\.direction === 'rtl'/)
  assert.match(tabs, /e\.key === 'Home'/)
  assert.match(d, /role: 'tabpanel', id: 'tf-activity-panel', 'aria-labelledby': `tf-activity-tab-\$\{tab\}`/)
})

test('P3: menu de @ como combobox, opção destacada anunciada, nada do menu no Tab', () => {
  const c = composer(fonte('./TarefaDetalhe.tsx'))
  assert.match(c, /role="combobox"/)
  assert.match(c, /aria-activedescendant=\{active \? `tf-mention-\$\{active\.id\}` : undefined\}/)
  assert.match(c, /<li key=\{b\.id\} id=\{`tf-mention-\$\{b\.id\}`\} role="option"/)
  const menu = c.slice(c.indexOf('id="tf-mention-list"'), c.indexOf('</ul>'))
  assert.doesNotMatch(menu, /<button/, 'botão dentro de option entrava na ordem do Tab')
})

test('P3: RTL com lados lógicos; textos e listas do catálogo; nomes acessíveis da busca e dos filtros', () => {
  const css = fonte('./tarefas.css')
  const regra = (sel: string) => css.slice(css.indexOf(`${sel} {`), css.indexOf('}', css.indexOf(`${sel} {`)))
  for (const sel of ['.tf-row-open', '.tf-composer-box']) {
    assert.doesNotMatch(regra(sel), /padding:\s*\S+\s+\S+\s+\S+\s+\S+;/, `${sel}: sem padding físico de 4 lados`)
    assert.match(regra(sel), /padding-inline:/)
  }
  const d = fonte('./TarefaDetalhe.tsx')
  assert.doesNotMatch(d, /\.join\(', '\)/, 'nomes juntados por Intl.ListFormat (joinNames), não por vírgula fixa')
  assert.doesNotMatch(d, /` · “\$\{/, 'título da sessão vem do catálogo')
  assert.match(d, /t\('projetos\.tarefas\.sessao\.titulo', \{ nome: name, titulo: title \}\)/)
  const v = fonte('./TarefasView.tsx')
  assert.match(v, /inputProps=\{\{ 'aria-label': t\('projetos\.tarefas\.buscar'\) \}\}/)
  assert.match(v, /label=\{t\('projetos\.tarefas\.filtroProjeto', \{ valor:/)
  assert.match(v, /label=\{t\('projetos\.tarefas\.filtroBot', \{ valor:/)
  assert.match(fonte('../../../components/Controls.tsx'), /title=\{title\} aria-label=\{label\} disabled=\{disabled\} aria-haspopup="menu"/, 'DropdownMenu repassa o nome')
  assert.match(fonte('./TarefaSheet.tsx'), /followCurrentPhase\(phases, m\.currentPhase, next\)/, 'a folha usa a fase atual por nome')
})
