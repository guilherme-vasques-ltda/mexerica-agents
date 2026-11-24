import assert from 'node:assert/strict'
import test from 'node:test'
import { RoutineDemoSession, savedRun } from './rotina-etapas.ts'

test('demo is off by default and cannot record intent', () => {
  const session = new RoutineDemoSession()
  assert.equal(session.enabled, false)
  assert.equal(session.continueOnce(), false)
  assert.equal(session.recorded, false)
})

test('resumes the saved snapshot after completed steps', () => {
  const session = new RoutineDemoSession(true)
  assert.equal(session.steps[0]?.state, 'done')
  assert.equal(session.steps[1]?.state, 'done')
  assert.equal(session.continueOnce(), true)
  assert.equal(session.steps[0]?.state, 'done')
  assert.equal(session.steps[2]?.state, 'done')
  assert.equal(session.steps[3]?.state, 'pending')
})

test('an uncertain effect blocks generic retry', () => {
  const steps = savedRun.map((step) => ({ ...step }))
  steps[2]!.state = 'done'
  steps[3]!.state = 'uncertain'
  const session = new RoutineDemoSession(true, steps)
  assert.equal(session.continueOnce(), false)
  assert.equal(session.recorded, false)
  assert.equal(session.steps[3]?.state, 'uncertain')
})

test('a rapid second click cannot record another intention', () => {
  const session = new RoutineDemoSession(true)
  assert.equal(session.continueOnce(), true)
  assert.equal(session.continueOnce(), false)
  assert.equal(session.steps.filter((step) => step.state === 'done').length, 3)
})
