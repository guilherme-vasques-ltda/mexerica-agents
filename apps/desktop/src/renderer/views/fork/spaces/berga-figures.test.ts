import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ACESSORIOS, CORES, OBJETOS, OLHOS, ROUPAS } from '../../../lib/mexeriquinhas/pecas.ts'
import { BERGA_CORES, BERGA_FIGURES, bergaAppearance } from './berga-figures.ts'

// D543 E1: as seis personalidades são só combinações de peças que o editor já tem;
// o Hub valida `aparencia` — a tela garante que nenhum id é inventado.

test('seis figuras do Berga, só com peças existentes no editor', () => {
  assert.equal(BERGA_FIGURES.length, 6)
  assert.deepEqual(BERGA_FIGURES.map((f) => f.id), ['simples', 'elegante', 'despojado', 'brabo', 'praieiro', 'curioso'])
  const ids = (list: readonly { id: string }[]) => new Set(list.map((o) => o.id))
  const olhos = ids(OLHOS), roupas = ids(ROUPAS), acessorios = ids(ACESSORIOS), objetos = ids(OBJETOS)
  for (const f of BERGA_FIGURES) {
    assert.ok(olhos.has(f.olhos), `${f.id}.olhos`)
    assert.ok(roupas.has(f.roupa), `${f.id}.roupa`)
    assert.ok(acessorios.has(f.acessorio), `${f.id}.acessorio`)
    assert.ok(objetos.has(f.objeto), `${f.id}.objeto`)
  }
})

test('seis cores do seletor e aparência no contrato Simples v2', () => {
  assert.deepEqual([...BERGA_CORES], CORES.map((c) => c.id))
  for (const cor of BERGA_CORES) {
    const a = bergaAppearance(BERGA_FIGURES[0]!, cor)
    assert.equal(a.v, 2)
    assert.equal(a.estilo, 'simples')
    assert.equal(a.aparencia.cor, cor)
    assert.ok(!('modeloId' in a), 'sem modeloId: figuras são personalidades, não modelos')
  }
})
