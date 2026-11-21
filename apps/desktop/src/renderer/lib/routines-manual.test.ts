import assert from 'node:assert/strict'
import { test } from 'node:test'
import { resolve } from 'node:path'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

function idiomaSalvo(idioma: string) {
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: (chave: string) => (chave === 'appLanguage' ? JSON.stringify(idioma) : null), setItem() {}, removeItem() {},
  } })
}

test('rotina «só quando eu mandar»: lista, painel e faixa do calendário mostram o botão e o texto, sem horário', async () => {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    addEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }), codync: { platform: 'linux' },
  } })
  idiomaSalvo('pt-BR')
  // O Icon assina um manifesto no cliente e não renderiza no servidor: aqui vira um espaço vazio.
  const iconeVazio = { name: 'icone-vazio', enforce: 'pre' as const, load: (id: string) => (id.endsWith('/components/Icon.tsx') ? 'export const Icon = () => null' : null) }
  const vite = await createServer({ configFile: false, root: process.cwd(), server: { middlewareMode: true }, plugins: [iconeVazio], resolve: { alias: { '@shared': resolve('src/shared') } } })
  try {
    const { isManual, isOnDemand } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/model.ts')
    const { RoutineList } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/RoutineList.tsx')
    const { OnDemandBand } = await vite.ssrLoadModule('/src/renderer/views/rotinas-central/parts.tsx')
    const { routineLine } = await vite.ssrLoadModule('/src/renderer/views/routines/RoutinesView.tsx')
    const manual = { id: 'm', botId: 'b', name: 'Fechar caixa', enabled: true, summary: 'Só quando eu mandar', triggers: [], triggerDescriptions: [], nextRunAt: null }
    const timed = { id: 't', botId: 'b', name: 'Diária', enabled: true, summary: 'Todo dia às 9h', triggers: [{ type: 'cron' }], triggerDescriptions: ['Todo dia'], nextRunAt: 1 }
    assert.equal(isManual(manual), true)
    assert.equal(isManual(timed), false)
    assert.equal(isOnDemand(manual), false, 'sem gatilho não é webhook nem evento')
    const list = (routines: unknown[], over: Record<string, unknown> = {}) => renderToStaticMarkup(React.createElement(RoutineList, { globalPause: false, routines, runs: [], bots: new Map(), invalid: new Set(), busy: null, disabled: false, onOpen() {}, onToggle() {}, onRun() {}, ...over }))
    const html = list([manual, timed])
    assert.match(html, /aria-label="Só quando eu mandar"/)
    assert.match(html, /aria-label="Rodar agora: [^"]*Fechar caixa[^"]*"/)
    assert.equal(html.match(/Rodar agora:/g)?.length, 2, 'título e rótulo só da rotina manual')
    assert.doesNotMatch(html, /Sem próximo horário/)
    assert.doesNotMatch(list([timed]), /Rodar agora:/)
    assert.match(list([{ ...manual, enabled: false }]), /Pausada/)
    assert.doesNotMatch(list([{ ...manual, enabled: false }]), /Rodar agora:/)
    assert.match(list([manual], { globalPause: true }), /<button[^>]*rc-row-run[^>]*disabled|<button[^>]*disabled[^>]*rc-row-run/)
    assert.deepEqual(routineLine(manual, [], 'pt-BR'), { text: 'Só quando eu mandar' })
    assert.equal(routineLine(timed, [], 'pt-BR').text, 'Todo dia')
    const band = renderToStaticMarkup(React.createElement(OnDemandBand, { onDemand: [], manual: [manual], frequent: [], bots: new Map(), onOpen() {} }))
    assert.match(band, /Só quando eu mandar/)
  } finally { await vite.close() }
})
