import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

/** DOM real dos controles: o modo desligado não recebe wrappers, classes, atributos ou desenhos extras. */
test('D530 R1: controles compartilhados preservam o DOM de base e gomo é opt-in de tarefa', async () => {
  const storage = new Map([['appLanguage', '"pt-BR"']])
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  } })
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {},
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  } })
  const vite = await createServer({ configFile: false,
    root: fileURLToPath(new URL('../../../', import.meta.url)),
    server: { middlewareMode: true }, resolve: { alias: { '@shared': fileURLToPath(new URL('../../shared', import.meta.url)) } },
  })
  try {
    const { Spinner, ProgressBar } = await vite.ssrLoadModule('/src/renderer/components/Controls.tsx')
    const { CascaVazia } = await vite.ssrLoadModule('/src/renderer/components/toques-mexerica/Toques.tsx')
    const { UsageBar } = await vite.ssrLoadModule('/src/renderer/views/usage/UsageBar.tsx')
    const { toquesMexericaPref } = await vite.ssrLoadModule('/src/renderer/lib/toques-mexerica.ts')
    const render = (component: React.ComponentType<any>, props: object) => renderToStaticMarkup(React.createElement(component, props))
    toquesMexericaPref.set(false)
    const spinner = render(Spinner, { size: 14 })
    assert.match(spinner, /^<svg class="spinner" width="14" height="14" viewBox="0 0 14 14" aria-label="Carregando" role="img"><circle /)
    assert.equal((spinner.match(/<svg/g) ?? []).length, 1)
    assert.doesNotMatch(spinner, /<span|mx-|aria-hidden/)
    const originalProgress = '<div class="progress-track usage-bar" role="progressbar" aria-label="37% used" aria-valuemin="0" aria-valuemax="100" aria-valuenow="37"><div class="progress-fill" style="transform:scaleX(0.37);background:var(--accent)"></div></div>'
    assert.equal(render(ProgressBar, { value: .37, label: '37% used', fill: 'var(--accent)', className: 'usage-bar' }), originalProgress)
    const empty = render(CascaVazia, { fallback: React.createElement('i', { 'data-original': 'empty' }) })
    assert.equal(empty, '<i data-original="empty"></i>')
    const taskOff = render(ProgressBar, { taskProgress: true, value: .37, label: '37% used', fill: 'var(--accent)', className: 'usage-bar' })
    assert.equal(taskOff, originalProgress)
    const usageOff = render(UsageBar, { percent: 37, tint: 'var(--accent)' })
    toquesMexericaPref.set(true)
    assert.equal(render(UsageBar, { percent: 37, tint: 'var(--accent)' }), usageOff)
    assert.equal(render(ProgressBar, { value: .37, label: '37% used', fill: 'var(--accent)', className: 'usage-bar' }), originalProgress)
    assert.match(render(ProgressBar, { taskProgress: true, value: .37, label: 'Download' }), /mx-task-progress/)
    assert.match(render(ProgressBar, { taskProgress: true, value: .37, label: 'Download' }), /mx-wedge/)
    assert.doesNotMatch(render(ProgressBar, { taskProgress: true, value: NaN, label: 'Download' }), /aria-valuenow|mx-wedge/)
    const loading = render(Spinner, { size: 14 })
    assert.match(loading, /class="mx-loader" style="width:14px;height:14px"/)
    assert.match(loading, /class="spinner"/)
    assert.match(loading, /mx-pith/)
    assert.equal((loading.match(/role="img"/g) ?? []).length, 1)
  } finally {
    await vite.close()
  }
})
