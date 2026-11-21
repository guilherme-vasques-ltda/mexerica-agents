import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

/** A escolha de idioma salva (lib/prefs lê o localStorage ao carregar); `null`: nunca escolhido. */
function idiomaSalvo(idioma: string | null) {
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: (chave: string) => (chave === 'appLanguage' && idioma ? JSON.stringify(idioma) : null), setItem() {}, removeItem() {},
  } })
}

test('lista e grade reais suspendem previsões e preservam execuções e enabled', async () => {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    addEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  } })
  idiomaSalvo('pt-BR')
  const vite = await createServer({ configFile: false, root: process.cwd(), server: { middlewareMode: true }, resolve: { alias: { '@shared': resolve('src/shared') } } })
  try {
    const { calendarItems } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/model.ts')
    const { RoutineList } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/RoutineList.tsx')
    const { TimeGrid } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/TimeGrid.tsx')
    const { GlobalPause } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/GlobalPause.tsx')
    const unavailable = renderToStaticMarkup(React.createElement(GlobalPause, { model: { paused: null, busy: false, loading: true, error: null }, offline: false }))
    assert.match(unavailable, /<button[^>]*disabled/)
    const day = new Date(2026, 10, 2).setHours(0, 0, 0, 0)
    const at = day + 12 * 3600_000
    const routine = { id: 'r', botId: 'b', name: 'Rotina', enabled: true, summary: 'Diária', triggers: [{ type: 'cron' }], nextRunAt: at }
    const overview = { routines: [routine], occurrences: [{ routineId: 'r', botId: 'b', at, trigger: 0, enabled: true }], runs: [], dense: [] }
    const list = (paused: boolean | null, runs: unknown[] = []) => renderToStaticMarkup(React.createElement(RoutineList, { globalPause: paused, routines: [routine], runs, bots: new Map(), invalid: new Set(), busy: null, disabled: false, onOpen() {}, onToggle() {}, onRun() {} }))
    const grid = (paused: boolean | null) => {
      const items = calendarItems(overview, [routine], paused)
      assert.equal(items[0].paused, paused !== false)
      return renderToStaticMarkup(React.createElement(TimeGrid, { days: [day], items, bots: new Map(), today: day, now: at, ready: true, disabled: false, onOpen() {}, onOpenDay() {}, onCreate() {} }))
    }
    assert.match(list(true), /Suspensa pela pausa do Hub/)
    assert.doesNotMatch(list(true), /Próxima:/)
    assert.match(grid(true), /Suspensa pela pausa do Hub/)
    assert.match(list(false), /Próxima:/)
    assert.doesNotMatch(grid(false), /Suspensa pela pausa do Hub/)
    assert.match(list(null), /Estado da pausa indisponível/)
    assert.match(grid(null), /Estado da pausa indisponível/)
    for (const [status, label] of [['starting', 'Começando'], ['running', 'Em execução'], ['recovering', 'Retomando']]) {
      assert.match(list(true, [{ id: 'run', routineId: 'r', status }]), new RegExp(label!))
    }
    const started = { id: 'run', routineId: 'r', botId: 'b', status: 'running', createdAt: at }
    const active = calendarItems({ ...overview, runs: [started] }, [routine], true)
    assert.equal(active.find((item: { run: unknown }) => item.run)?.paused, false)
    assert.equal(routine.enabled, true)
  } finally { await vite.close() }
})

test('I-023: lista e grade reais mostram o desfecho; ✓ só com declaração de sucesso', async () => {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    addEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  } })
  idiomaSalvo('pt-BR')
  const vite = await createServer({ configFile: false, root: process.cwd(), server: { middlewareMode: true }, resolve: { alias: { '@shared': resolve('src/shared') } } })
  try {
    const { calendarItems } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/model.ts')
    const { RoutineList } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/RoutineList.tsx')
    // A grade decide ✓ e alerta por `itemClass` (o ícone não renderiza no servidor); o texto por `itemLabel`.
    const { itemClass, itemLabel } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/parts.tsx')
    const day = new Date(2026, 10, 2).setHours(0, 0, 0, 0)
    const at = day + 9 * 3600_000
    const routine = { id: 'r', botId: 'b', name: 'Rotina', enabled: true, summary: 'Diária', triggers: [{ type: 'cron' }], nextRunAt: null }
    const run = (outcome: unknown, status = 'succeeded') => ({ id: 'run', routineId: 'r', botId: 'b', status, createdAt: at, outcome })
    const grid = (r: unknown) => {
      const [item] = calendarItems({ routines: [routine], occurrences: [], runs: [r], dense: [] }, [routine])
      return `rc-card ${itemClass(item)} ${itemLabel(item, 'Bot')}`
    }
    const list = (r: unknown) => renderToStaticMarkup(React.createElement(RoutineList, { globalPause: false, routines: [routine], runs: [r], bots: new Map(), invalid: new Set(), busy: null, disabled: false, onOpen() {}, onToggle() {}, onRun() {} }))
    assert.match(grid(run({ version: 1, kind: 'noNews', source: 'declared' })), /rc-card[^"]* ok/)
    for (const old of [run(undefined), run({ version: 1, kind: 'unconfirmed', source: 'none', reason: 'noDeclaration' })]) {
      const html = grid(old)
      assert.doesNotMatch(html, /rc-card[^"]* ok|Concluída/)
      assert.match(html, /Terminou|Sem confirmação/)
    }
    assert.match(grid(run({ version: 1, kind: 'needsOwner', source: 'declared', summary: 'Falta o código' }, 'failed')), /rc-card[^"]* attention[\s\S]*Precisa de você/)
    assert.match(list(run({ version: 1, kind: 'needsOwner', source: 'declared', summary: 'x' }, 'failed')), /A última execução precisa de você/)
    assert.match(list(run({ version: 1, kind: 'unconfirmed', source: 'none', reason: 'noDeclaration' })), /A última execução terminou sem confirmação/)
    assert.match(list(run({ version: 1, kind: 'failed', source: 'declared', summary: 'x' }, 'failed')), /A última execução falhou/)
    // R1: origem desconhecida não dá ✓ nem na grade real.
    assert.doesNotMatch(grid(run({ version: 1, kind: 'reported', source: 'unknown', summary: 'x' })), /rc-card[^"]* ok/)
    assert.doesNotMatch(grid(run({ version: 1, kind: 'noNews', source: 'unknown' })), /rc-card[^"]* ok/)
    // R2: a lista real conserva o que falta e a atribuição da falha declarada (dica e leitor de tela).
    const needs = list(run({ version: 1, kind: 'needsOwner', source: 'declared', summary: 'Falta aprovar o pagamento' }, 'failed'))
    assert.match(needs, /title="O bot precisa de algo seu: Falta aprovar o pagamento"/)
    assert.match(needs, /rc-sr-only[^>]*>\. O bot precisa de algo seu: Falta aprovar o pagamento/)
    const declared = list(run({ version: 1, kind: 'failed', source: 'declared', summary: 'Site fora do ar' }, 'failed'))
    assert.match(declared, /title="Segundo o bot: Site fora do ar"/)
    // O painel do bot usa a mesma linha (`routineLine` do RoutinesView real); o editor importado lê a plataforma.
    Object.assign(globalThis.window, { codync: { platform: 'linux' } })
    const { routineLine } = await vite.ssrLoadModule('/src/renderer/views/routines/RoutinesView.tsx')
    const panel = (r: unknown) => routineLine({ ...routine, triggerDescriptions: ['Todo dia'] }, [r], 'pt-BR')
    assert.deepEqual(panel(run({ version: 1, kind: 'needsOwner', source: 'declared', summary: 'Falta aprovar o pagamento' }, 'failed')),
      { text: 'Última execução: precisa de você', detail: 'Precisa de você. O bot precisa de algo seu: Falta aprovar o pagamento' })
    assert.equal(panel(run({ version: 1, kind: 'failed', source: 'declared', summary: 'Site fora do ar' }, 'failed')).detail, 'Não conseguiu. Segundo o bot: Site fora do ar')
    assert.equal(panel(run({ version: 1, kind: 'reported', source: 'unknown', summary: 'x' })).text, 'Última execução: sem confirmação')
    assert.equal(panel(run({ version: 1, kind: 'reported', source: 'declared', summary: 'x' })).text, 'Todo dia')
  } finally { await vite.close() }
})

test('DON-917: sem escolha a tela de Rotinas sai em inglês; ar-AE declara rtl e o código fica LTR isolado', async () => {
  for (const idioma of [null, 'ar-AE'] as const) {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {
      addEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }), codync: { platform: 'linux' },
    } })
    idiomaSalvo(idioma)
    const vite = await createServer({ configFile: false, root: process.cwd(), server: { middlewareMode: true }, resolve: { alias: { '@shared': resolve('src/shared') } } })
    try {
      const { RoutineList } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/RoutineList.tsx')
      const { GlobalPause } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/GlobalPause.tsx')
      const routine = { id: 'r', botId: 'b', name: 'روتين ‏Daily', enabled: true, summary: 'Diária', triggers: [{ type: 'cron' }], nextRunAt: null }
      const lista = renderToStaticMarkup(React.createElement(RoutineList, { globalPause: true, routines: [routine], runs: [], bots: new Map(), invalid: new Set(), busy: null, disabled: false, onOpen() {}, onToggle() {}, onRun() {} }))
      // O nome é do usuário: a direção vem do próprio texto.
      assert.match(lista, /<span class="rc-row-name" dir="auto">/)
      // R1 do #570: o nome do bot na linha também é isolado.
      assert.match(lista, /<span class="rc-row-summary"><bdi dir="auto">/)
      // A expressão cron, a legenda dos campos, os fusos e a URL/chave do webhook são código: sempre LTR (o atributo
      // dir isola o trecho). Fonte, não render: o Icon do seletor de fuso não renderiza no servidor.
      const fonte = (arquivo: string) => readFileSync(resolve('src/renderer/views/routines', arquivo), 'utf8')
      assert.match(fonte('RoutineScheduleFields.tsx'), /routine-cron-input"\s*\/\/[^\n]*\n\s*dir="ltr"/)
      assert.match(fonte('RoutineScheduleFields.tsx'), /routine-cron-hint" dir="ltr"/)
      assert.equal(fonte('TimeZonePicker.tsx').match(/dir="ltr"/g)?.length, 4)
      assert.match(fonte('RoutineWebhookPanel.tsx'), /routine-webhook-text" dir="ltr"/)
      // Diálogos e painéis em portal no body declaram o idioma da tela; a confirmação passa o Cancelar do catálogo.
      for (const arquivo of ['RoutineEditorView.tsx', 'RoutineWebhookPanel.tsx']) {
        assert.match(fonte(arquivo), /cancel=\{t\('rotinas\.editor\.cancelar'\)\}\s*language=\{superficieIdioma\(idioma\)\}/)
      }
      assert.match(fonte('TimeZonePicker.tsx'), /<div\s*\{\.\.\.superficieIdioma\(idioma\)\}\s*className="zone-picker"/)
      // Lição da R1 do #572: texto do usuário com a direção dele (campos com dir="auto"; dentro de frase, entre FSI e PDI).
      assert.match(fonte('RoutineEditorView.tsx'), /<input className="routine-input" dir="auto" value=\{name\}/)
      const central = (arquivo: string) => readFileSync(resolve('src/renderer/views/rotinas-central', arquivo), 'utf8')
      assert.match(central('TimeGrid.tsx'), /<bdi dir="auto">\{botName\(bots, item\.routine\.botId\)\}<\/bdi>/)
      assert.match(central('parts.tsx'), /rc-chip-detail"><bdi dir="auto">\{botName/)
      assert.match(fonte('RoutineEditorView.tsx'), /<textarea className="routine-input" dir="auto" value=\{instruction\}/)
      const { texts } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/texts.ts')
      assert.ok(texts.noResultsFor('روتين').includes('\u2068روتين\u2069'))
      assert.ok(texts.forBot('Byte').includes('\u2068Byte\u2069'))
      // Checklist de autorrevisão, classe 2: o erro fica cru no estado; a tela traduz ao desenhar (nada de set…(errorText(e))).
      for (const arquivo of ['RoutineEditorView.tsx', 'RoutinesView.tsx', 'RoutineWebhookPanel.tsx']) assert.doesNotMatch(fonte(arquivo), /set\w*\(errorText\(/)
      assert.doesNotMatch(central('RoutinesCentralView.tsx') + central('model.ts'), /set\w*\(errorText\(/)
      // Classe 5: o fechar do cabeçalho vem do catálogo.
      assert.match(fonte('RoutineEditorView.tsx'), /closeTitle=\{t\('rotinas\.editor\.fechar'\)\}/)
      assert.match(central('RoutineSheet.tsx'), /closeTitle=\{texts\.close\}/)
      const { agendaDoHub } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/editor-textos.ts')
      const preview = { summary: 'At 09:00 on weekdays', resumo: 'Às 09:00 nos dias úteis' }
      assert.equal(agendaDoHub(preview, 'pt-BR'), 'Às 09:00 nos dias úteis')
      assert.equal(agendaDoHub(preview, idioma ?? 'en'), 'At 09:00 on weekdays', 'o resumo em pt-BR do Hub só aparece em pt-BR')
      const pausa = renderToStaticMarkup(React.createElement(GlobalPause, { model: { paused: true, busy: false, loading: false, error: null, setPaused() {} }, offline: false }))
      if (idioma === null) {
        assert.match(lista, /Suspended by the Hub pause/)
        assert.match(pausa, /Resume routines/)
      } else {
        assert.match(lista, /معلّقة بسبب إيقاف الـ Hub المؤقت/)
        assert.match(pausa, /استئناف المهام الروتينية/)
        const { superficieIdioma } = await vite.ssrLoadModule('/src/renderer/lib/fork-i18n/estado.ts')
        assert.deepEqual(superficieIdioma('ar-AE'), { lang: 'ar-AE', dir: 'rtl' })
      }
    } finally { await vite.close() }
  }
})
