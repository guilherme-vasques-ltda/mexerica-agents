import assert from 'node:assert/strict'
import { test } from 'node:test'
import { trackBody, type AnalyticsEvent } from '../../shared/analytics.ts'
import { AnalyticsQueue } from './analytics-queue.ts'

const settle = () => new Promise((r) => setImmediate(r))

test('eventos são descartados antes e depois de conectar o host', async () => {
  const queue = new AnalyticsQueue()
  const sent: AnalyticsEvent[] = []
  queue.track({ event: 'app_opened', at_login: false })
  queue.connect(async (e) => void sent.push(e))
  queue.track({ event: 'screen_viewed', screen: 'chat' })
  await settle()
  queue.connect(null)
  queue.track({ event: 'signed_out' })
  queue.connect(async (e) => void sent.push(e))
  await settle()
  assert.deepEqual(sent, [])
})

test('track sends the event name and its properties, from the desktop app', () => {
  assert.deepEqual(trackBody({ event: 'sign_in_failed', provider: 'google', reason: 'timeout' }), {
    event: 'sign_in_failed', provider: 'google', reason: 'timeout', app: 'desktop',
  })
  assert.deepEqual(trackBody({ event: 'window_opened' }), { event: 'window_opened', app: 'desktop' })
})
