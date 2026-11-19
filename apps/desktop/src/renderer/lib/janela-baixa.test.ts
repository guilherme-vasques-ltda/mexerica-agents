import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { JANELA_BAIXA } from './janela-baixa.ts'

// Auditoria D249, E4: a regra de altura é uma só. O app (rosto de 28 px, fatos em Detalhes, gaveta de Times,
// "Saiba mais" de Rotinas) e o CSS de cada tela mudam juntos, abaixo de 600 px de altura.

test('a janela baixa é abaixo de 600 px de altura', () => {
  assert.equal(JANELA_BAIXA, '(height < 600px)')
})

test('o CSS de cada tela usa a mesma consulta do app', () => {
  for (const css of ['../views/thread/janela-baixa.css', '../views/fork/times/times.css', '../views/rotinas-central/routines-central.css']) {
    const text = readFileSync(new URL(css, import.meta.url), 'utf8')
    const queries = [...text.matchAll(/@media\s+(\([^)]*height[^)]*\))/g)].map((m) => m[1])
    assert.ok(queries.length > 0, `${css} sem a regra de altura`)
    for (const q of queries) assert.equal(q, JANELA_BAIXA, css)
  }
})
