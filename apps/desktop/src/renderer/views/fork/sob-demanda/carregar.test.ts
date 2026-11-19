// #753: parte sob demanda que não carrega não derruba a janela (a raiz do React ficaria vazia).
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { carregarOuIndisponivel } from './carregar.ts'

test('arquivo da parte sumiu: devolve a parte vazia em vez de rejeitar; carregou: a parte de verdade', async () => {
  const vazia = () => null
  const real = () => null
  assert.equal((await carregarOuIndisponivel(async () => { throw new TypeError('Failed to fetch dynamically imported module') }, vazia)).default, vazia)
  assert.equal((await carregarOuIndisponivel(async () => real, vazia)).default, real)
})

test('todas as telas sob demanda passam pela carga que não derruba a janela', () => {
  const fonte = readFileSync(new URL('./telas.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /carregarOuIndisponivel<ComponentType<P>>\(carregar, Falhou\)/)
  assert.doesNotMatch(fonte, /lazy\(async/)
})

test('parte indisponível anuncia falha e oferece nova tentativa', () => {
  const fonte = readFileSync(new URL('./telas.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /sistema\.conversa\.parteIndisponivel/)
  assert.match(fonte, /sistema\.conversa\.tentarParte/)
  assert.match(fonte, /onClick=\{tentar\}/)
})
