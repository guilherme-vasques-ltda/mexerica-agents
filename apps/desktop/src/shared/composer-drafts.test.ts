// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ComposerDrafts } from './composer-drafts.ts'
import { originDraft, originReport } from './guard-hub.ts'

function storage() {
  const data = new Map<string, string>()
  return { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value) } }
}

test('drafts survive reopening with whitespace and isolate bots and accounts', () => {
  const disk = storage()
  const drafts = new ComposerDrafts(disk, 'account/computer')
  const text = '  中文\nunfinished 🐱\n'
  drafts.set('bot', text)
  drafts.set('other', 'other draft')
  const reopened = new ComposerDrafts(disk, 'account/computer')
  assert.equal(reopened.get('bot'), text)
  assert.equal(reopened.get('other'), 'other draft')
  assert.equal(new ComposerDrafts(disk, 'other-account/computer').get('bot'), '')
  assert.equal(new ComposerDrafts(disk, 'account/other-computer').get('bot'), '')
  reopened.set('bot', '')
  assert.equal(new ComposerDrafts(disk, 'account/computer').get('bot'), '')
  assert.equal(reopened.get('other'), 'other draft')
})

test('drafts saved before quote replies stay with their chat', () => {
  const disk = storage()
  disk.setItem('drafts', JSON.stringify([['["bot",null]', 'unsent'], ['["bot","root"]', 'aside'], ['other', 'new']]))
  const drafts = new ComposerDrafts(disk, 'drafts')
  assert.equal(drafts.get('bot'), 'unsent')
  assert.equal(drafts.get('other'), 'new')
  drafts.set('other', 'newer')
  assert.equal(disk.getItem('drafts'), JSON.stringify([['bot', 'unsent'], ['other', 'newer']]))
})

test('deleting a bot removes its draft, but preserves other bots', () => {
  const drafts = new ComposerDrafts(storage(), 'drafts')
  drafts.set('bot', 'main')
  drafts.set('other', 'keep')
  drafts.removeBot('bot')
  assert.equal(drafts.get('bot'), '')
  assert.equal(drafts.get('other'), 'keep')
})

test('malformed or unavailable persistence does not destroy editable in-memory text', () => {
  const disk = { getItem: () => '{bad', setItem: () => { throw new Error('full') } }
  const drafts = new ComposerDrafts(disk, 'drafts')
  let updates = 0
  const unsubscribe = drafts.subscribe(() => updates++)
  drafts.set('bot', 'kept')
  assert.equal(drafts.get('bot'), 'kept')
  assert.equal(updates, 1)
  unsubscribe()
  drafts.set('bot', 'next')
  assert.equal(updates, 1)
})

test('late edits from a retired store cannot overwrite a reopened conversation', () => {
  const disk = storage()
  const previous = new ComposerDrafts(disk, 'account/computer')
  previous.set('bot', 'old text')
  previous.retire()
  const current = new ComposerDrafts(disk, 'account/computer')
  current.set('bot', 'new text')
  previous.set('bot', 'late edit')
  previous.clear()
  assert.equal(new ComposerDrafts(disk, 'account/computer').get('bot'), 'new text')
})

test('rollback lê strings na chave antiga; origens ficam em armazenamento separado', () => {
  const disk = storage(), suffix = JSON.stringify(['conta', 'computador'])
  const key = `codync-drafts-v1-${suffix}`, originKey = `codync-draft-origins-v1-${suffix}`
  const drafts = new ComposerDrafts(disk, key)
  drafts.edit('bot', 'colado', 'pasted')
  const legacy: [string, string][] = JSON.parse(disk.getItem(key)!)
  assert.deepEqual(legacy, [['bot', 'colado']])
  assert.ok(disk.getItem(originKey))
  // O app anterior pode salvar as strings sem conhecer a nova chave.
  disk.setItem(key, JSON.stringify(legacy))
  assert.equal(new ComposerDrafts(disk, key).origin('bot')?.segments[0]?.kind, 'pasted')
  drafts.removeBot('bot')
  assert.deepEqual(JSON.parse(disk.getItem(originKey)!), [])
})

test('metadados de rascunho com outro texto do mesmo comprimento ficam desconhecidos', () => {
  const disk = storage(), key = 'codync-drafts-v1-conta'
  const drafts = new ComposerDrafts(disk, key)
  drafts.edit('bot', 'antigo', 'typed')
  disk.setItem(key, JSON.stringify([['bot', 'outros']]))
  const reopened = new ComposerDrafts(disk, key)
  assert.equal(reopened.get('bot'), 'outros')
  assert.equal(originReport(reopened.get('bot'), reopened.origin('bot'), crypto.randomUUID()).segments[0]?.kind, 'unknown')
})

test('rascunho de 2 MiB grava o corpo uma vez e restaura origens por comprimento e hash', () => {
  const data = new Map<string, string>(), text = 'x'.repeat(2 * 1024 * 1024)
  let errors = 0
  const disk = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      const size = [...data].reduce((sum, [k, v]) => sum + (k === key ? 0 : v.length), value.length)
      if (size > 3 * 1024 * 1024) throw new Error('QuotaExceededError')
      data.set(key, value)
    },
  }
  const drafts = new ComposerDrafts(disk, 'drafts', () => errors++)
  drafts.edit('bot', text, 'pasted')
  assert.equal(errors, 0)
  const metadata = JSON.parse(disk.getItem('drafts-origins-v1')!)[0][1]
  assert.equal(metadata.text, undefined)
  assert.equal(metadata.textLength, text.length)
  assert.match(metadata.textHash, /^[0-9a-f]{8}$/)
  assert.ok(disk.getItem('drafts-origins-v1')!.length < 1024)
  const reopened = new ComposerDrafts(disk, 'drafts')
  assert.equal(reopened.get('bot'), text)
  assert.deepEqual(reopened.origin('bot')?.segments, drafts.origin('bot')?.segments)
  disk.setItem('drafts', JSON.stringify([['bot', text + '!']]))
  assert.equal(new ComposerDrafts(disk, 'drafts').origin('bot'), undefined)
})

test('metadados antigos sem hash ou com hash inválido não promovem o rascunho', () => {
  const disk = storage(), text = 'legado'
  disk.setItem('drafts', JSON.stringify([['bot', text]]))
  for (const metadata of [
    { text, origin: originDraft(text, 'typed') },
    { textLength: text.length, textHash: 'inválido', origin: originDraft(text, 'typed') },
  ]) {
    disk.setItem('drafts-origins-v1', JSON.stringify([['bot', metadata]]))
    const reopened = new ComposerDrafts(disk, 'drafts')
    assert.equal(originReport(text, reopened.origin('bot'), crypto.randomUUID()).segments[0]?.kind, 'unknown')
  }
})

test('anexos usam snapshot estável, isolam stores e não são persistidos', () => {
  const disk = storage(), drafts = new ComposerDrafts(disk, 'account/computer/space')
  const file = { id: 'image', name: 'image.png', data: new Uint8Array([137, 80, 78, 71]) }
  assert.equal(drafts.files('a'), drafts.files('a'))
  drafts.set('a', 'texto')
  const persisted = disk.getItem('account/computer/space')
  drafts.setFiles('a', [file])
  assert.equal(drafts.files('a'), drafts.files('a'))
  assert.equal(drafts.files('a')[0], file)
  assert.deepEqual(drafts.files('b'), [])
  for (const key of ['other-account/computer/space', 'account/other-computer/space', 'account/computer/other-space', 'account/computer/space']) {
    assert.deepEqual(new ComposerDrafts(disk, key).files('a'), [])
  }
  assert.equal(disk.getItem('account/computer/space'), persisted)
})

test('remoção, clear, envio e retire invalidam leituras pendentes sem ressuscitar anexos', () => {
  for (const action of ['removeBot', 'clear', 'send', 'retire'] as const) {
    const drafts = new ComposerDrafts(storage(), 'drafts')
    const file = { id: 'file', name: 'file.png', data: new Uint8Array([1]) }
    drafts.setFiles('a', [file])
    drafts.setFiles('b', [file])
    const accept = drafts.fileGuard('a')
    if (action === 'removeBot') drafts.removeBot('a')
    else if (action === 'send') drafts.setFiles('a', [])
    else drafts[action]()
    assert.equal(accept(), false, action)
    assert.deepEqual(drafts.files('a'), [])
    if (action === 'removeBot' || action === 'send') assert.equal(drafts.files('b')[0], file)
    else assert.deepEqual(drafts.files('b'), [])
    if (action === 'retire') {
      drafts.setFiles('a', [file])
      assert.equal(drafts.fileGuard('a')(), false)
    } else {
      assert.equal(drafts.fileGuard('a')(), true)
      assert.equal(accept(), false)
    }
  }
})
