import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import { loadMainModule } from './test-support/main-module.ts'

test('cloud falso não registra manipulador de requisição', () => {
  let registrations = 0
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: false } },
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: {} },
    electron: { app: {}, safeStorage: {}, ipcMain: { handle: () => { registrations++ } } },
  }, { Buffer })
  cloud.registerCloud({} as never)
  assert.equal(registrations, 0)
})
