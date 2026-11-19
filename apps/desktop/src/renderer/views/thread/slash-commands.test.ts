import assert from 'node:assert/strict'
import { test } from 'node:test'
import { groupFastMode } from '../../../shared/fast-mode.ts'
import { normalizeBot } from '../../../shared/models.ts'
import { tradutor } from '../../lib/fork-i18n/index.ts'

const en = tradutor('en')
import { builtInCommand, returnRuns, slashCommands, slashQuery } from './slash-commands.ts'

const installed = [
  { id: 'pdf', name: 'PDF', description: 'PDFs' },
  { id: 'code-review', name: 'Code Review', description: '' },
  { id: 'xlsx', name: 'xlsx', description: '' },
]

test('a leading slash opens the list until the name ends', () => {
  assert.equal(slashQuery('/'), '')
  assert.equal(slashQuery('/pd'), 'pd')
  assert.equal(slashQuery('/pdf merge'), null)
  assert.equal(slashQuery('hi /pdf'), null)
})

test('built-in commands come first, then only the bot\'s own skills, by name or title', () => {
  const bot = normalizeBot({ id: 'b', skills: ['pdf', 'code-review'], fastModeSupport: 'offered' })
  assert.deepEqual(slashCommands(bot, '', installed).map((c) => c.name), ['fast', 'pdf', 'code-review'])
  assert.deepEqual(slashCommands(bot, 'review', installed).map((c) => c.name), ['code-review'])
  assert.deepEqual(slashCommands(bot, 'new', installed), [], 'a new session is in the bot\'s menu')
  assert.deepEqual(slashCommands(normalizeBot({ id: 'b', skills: ['pdf'] }), 'xl', installed), [])
  assert.deepEqual(slashCommands(normalizeBot({ id: 'b' }), '', []).map((c) => c.name), ['fast'], 'offered until the agent says otherwise')
  assert.deepEqual(slashCommands(normalizeBot({ id: 'b', fastModeSupport: 'unsupported' }), '', []), [], 'no Fast mode on offer')
})

test('a group lists its bots\' commands: Fast mode for all of them, then every skill one of them has', () => {
  const group = normalizeBot({ id: 'g', kind: 'group', members: ['a', 'b'] })
  const a = normalizeBot({ id: 'a', name: 'Ada', skills: ['pdf'], fastModeSupport: 'offered' })
  const b = normalizeBot({ id: 'b', name: 'Bo', skills: ['pdf', 'xlsx'], fastModeSupport: 'unsupported' })
  assert.deepEqual(slashCommands(group, '', installed, [a, b]).map((c) => c.name), ['fast', 'pdf', 'xlsx'])
  assert.deepEqual(slashCommands(group, '', installed, [b]).map((c) => c.name), ['pdf', 'xlsx'], 'no bot here has Fast mode')
  assert.equal(builtInCommand(group, '/fast', [a, b])?.action.kind, 'fast')
  assert.equal(builtInCommand(group, '/fast', [b]), null)

  const both = [a, normalizeBot({ ...b, fastModeSupport: 'offered' })]
  assert.equal(groupFastMode(en, both)?.state, 'off')
  assert.equal(groupFastMode(en, [{ ...a, fastMode: true }, both[1]!])?.state, 'off', 'on once every bot has it on')
  const on = both.map((m) => ({ ...m, fastMode: true }))
  assert.equal(groupFastMode(en, on)?.state, 'on')
  const refused = groupFastMode(en, [on[0]!, { ...on[1]!, fastModeSupport: 'refused', fastModeIssue: 'quota' }])
  assert.equal(refused?.state, 'refused')
  assert.match(refused?.detail ?? '', /^Bo: the agent turned it off: quota/)
  assert.match(groupFastMode(tradutor('pt-BR'), [on[0]!, { ...on[1]!, fastModeSupport: 'refused', fastModeIssue: 'quota' }])?.detail ?? '', /^Bo: o agente desligou: quota/)
  assert.deepEqual(groupFastMode(en, [a, b])?.members.map((m) => m.id), ['a'], 'only the bots that have it')
})

test('sending just a built-in command runs it; anything else is a message', () => {
  const bot = normalizeBot({ id: 'b', fastModeSupport: 'offered' })
  assert.equal(builtInCommand(bot, '/fast ')?.action.kind, 'fast')
  assert.equal(builtInCommand(bot, '/new'), null, 'reaches the agent as typed')
  assert.equal(builtInCommand(bot, '/fast please'), null)
  assert.equal(builtInCommand(normalizeBot({ id: 'b', fastModeSupport: 'unsupported' }), '/fast'), null, 'without Fast mode it reaches the agent')
})

test('Return runs the highlighted command, but sends a skill already typed out', () => {
  const bot = normalizeBot({ id: 'b', skills: ['pdf'], fastModeSupport: 'offered' })
  const [fast, pdf] = slashCommands(bot, '', installed)
  assert.equal(returnRuns(fast!, '/'), true)
  assert.equal(returnRuns(pdf!, '/pd'), true)
  assert.equal(returnRuns(pdf!, '/pdf'), false)
})

test('"Skills no menu /" off: only the built-in commands; a typed skill still sends as text', () => {
  const bot = normalizeBot({ id: 'b', skills: ['pdf', 'code-review'], fastModeSupport: 'offered' })
  assert.deepEqual(slashCommands(bot, '', installed, [], false).map((c) => c.name), ['fast'])
  assert.deepEqual(slashCommands(bot, 'pd', installed, [], false), [], 'no list over /pd: Return sends it')
  const group = normalizeBot({ id: 'g', kind: 'group', members: ['a'] })
  const a = normalizeBot({ id: 'a', skills: ['pdf'], fastModeSupport: 'offered' })
  assert.deepEqual(slashCommands(group, '', installed, [a], false).map((c) => c.name), ['fast'])
  assert.equal(builtInCommand(bot, '/pdf'), null, '/pdf is not a built-in: it goes to the host')
})
