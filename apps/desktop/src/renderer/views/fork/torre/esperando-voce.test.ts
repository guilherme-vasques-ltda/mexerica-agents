import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const view = readFileSync(new URL('./EsperandoVoceList.tsx', import.meta.url), 'utf8')
const tower = readFileSync(new URL('./TorreView.tsx', import.meta.url), 'utf8')

test('canonical attention list exposes unavailable and coverage gaps without false zero', () => {
  assert.match(view, /!queue .*indisponivel/)
  assert.match(view, /coverageGaps/)
  assert.match(view, /sourceLabel\(gap\.source/)
  assert.match(view, /queue \? <span>.*: null/)
  assert.match(view, /queue && items.length === 0 && !queue.coverageGaps.length/)
})

test('resolved rows are collapsed and only render translated labels', () => {
  assert.match(view, /<details className="esperando-voce-resolvidos">/)
  assert.match(view, /kindLabel\(item\.kind\)/)
  assert.doesNotMatch(view, /t\(item\.titleKey\)/)
})

test('canonical rows reuse Torre conversation controls and suppress duplicate rows', () => {
  assert.match(tower, /pendingView\(responder\?\.entry/)
  assert.match(tower, /<QuestionItem/)
  assert.match(tower, /canonicalIds/)
  assert.match(tower, /goTo\(attention\.target\.conversation/)
})
