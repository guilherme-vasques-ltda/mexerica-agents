import type { Bot, EntryData, Usage } from '@shared/models'
import { configDaFrutinha, configDoPrincipal } from '../../shared/fork-frutinhas.ts'

export const workplaceSimulation = 'Simulação · dados fictícios'

// The website demo's story: four bots on a small `pace` project (an isoWeek / weeklyTotals bug),
// the "Lançamento" group, replies quoting earlier messages and a pending approval. Same story as web/public/screens.

export const COMPUTER_ID = 'demo-mac'
export const HOST_NAME = 'MacBook Pro'
const CWD = '/Users/usuario/code/pace'

export type SeedBot = Pick<Bot, 'id' | 'name' | 'description' | 'avatarColor' | 'avatarShape' | 'backend' | 'model'> &
  Partial<Pick<Bot, 'kind' | 'members' | 'permission' | 'status' | 'activity' | 'unread' | 'avatarConfig'>>

export const bots: SeedBot[] = [
  // D486: o Berga usa o preset do principal (selo sparkle): a mascote Mexerica.
  { id: 'sprint', name: 'Berga', description: 'Leads the team and owns the pace math', avatarColor: 'orange', avatarShape: 'squircle', avatarConfig: configDoPrincipal(), backend: 'claude', model: 'opus', permission: 'ask', status: 'needsInput', activity: 'Needs approval: Edit src/pace.js', unread: 1 },
  { id: 'byte', name: 'Byte', description: 'Finds things in the repo and builds them', avatarColor: 'green', avatarShape: 'cloud', backend: 'codex', model: null, unread: 1, status: 'working', activity: 'Adding the invalid-date guard' },
  // D478: Crivo (QA) e Byte (Dev) dariam a mesma frutinha; o revisor fica com a mais próxima distinta, Pesquisa.
  { id: 'crivo', name: 'Crivo', description: 'Reviews every change before it ships', avatarColor: 'violet', avatarShape: 'blob', avatarConfig: configDaFrutinha('pesquisa'), backend: 'claude', model: 'sonnet', unread: 1 },
  { id: 'prosa', name: 'Prosa', description: 'Writes the release notes', avatarColor: 'blue', avatarShape: 'tablet', backend: 'codex', model: null, unread: 1 },
  { id: 'lancamento', kind: 'group', members: ['sprint', 'byte', 'crivo'], name: 'Lançamento', description: 'Getting weeklyTotals out', avatarColor: 'gray', avatarShape: 'blob', backend: '', model: null },
]

export const cwd = CWD

export interface SeedEntry {
  id: string
  botId: string
  kind: string
  /** Minutes ago. */
  at: number
  data: EntryData
}

/** `quoting`: the bot reply it answers, quoted like the host does. */
const user = (id: string, botId: string, at: number, text: string, quoting?: SeedEntry): SeedEntry => ({
  id, botId, kind: 'user', at,
  data: { text, status: 'sent', ...(quoting ? { replyTo: { id: quoting.id, author: quoting.data.author ?? botId, text: quoting.data.text ?? '' } } : {}) },
})
const reply = (id: string, botId: string, author: string, at: number, text: string): SeedEntry => ({ id, botId, kind: 'agent', at, data: { text, final: true, author } })
const thought = (id: string, botId: string, at: number, text: string): SeedEntry => ({ id, botId, kind: 'thought', at, data: { text, author: botId } })
const tool = (id: string, botId: string, at: number, title: string, toolKind: string, output = ''): SeedEntry => ({ id, botId, kind: 'tool', at, data: { title, toolKind, status: 'completed', output, author: botId } })

const weeklyPatch = [
  '@@ -18,3 +18,15 @@ export function isoWeek(date) {',
  ' ',
  '+export function weeklyTotals(runs) {',
  '+  const weeks = {}',
  '+  for (const run of runs) {',
  '+    const key = isoWeek(run.date)',
  '+    const week = (weeks[key] ??= { distance: 0, time: 0 })',
  '+    week.distance += run.distance',
  '+    week.time += run.time',
  '+  }',
  '+  return weeks',
  '+}',
].join('\n')

const isoWeekPatch = [
  '@@ -4,9 +4,10 @@ export function isoWeek(date) {',
  '   const d = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()))',
  '-  const year = d.getUTCFullYear()',
  '   d.setUTCDate(d.getUTCDate() + 4 - (d.getUTCDay() || 7))',
  '+  // The ISO year is the year of the Thursday of this week.',
  '+  const year = d.getUTCFullYear()',
  '   const start = new Date(Date.UTC(year, 0, 1))',
  '   const week = Math.ceil(((d - start) / 86400000 + 1) / 7)',
  '   return `${year}-W${String(week).padStart(2, "0")}`',
].join('\n')

const prosaSummary = reply('l2', 'prosa', 'prosa', 40, 'Added `weeklyTotals` to group run distance and time by ISO week, with a test for the 2026 to 2027 rollover. Crivo found no blockers. An invalid-date guard is being added in Lançamento.')
const byteGuard = reply('g3', 'lancamento', 'byte', 19, 'Added the guard: `weeklyTotals` now throws a `RangeError` on an invalid date, and all 5 tests pass. Weeks are still grouped in UTC, so say if they should follow the runner\'s local time instead.')

export const entries: SeedEntry[] = [
  // Byte builds weeklyTotals.
  user('s1', 'byte', 58, 'Add a weeklyTotals(runs) helper that sums distance and time per ISO week, with a test.'),
  thought('s2', 'byte', 58, 'isoWeek already lives in src/pace.js, so weeklyTotals can key on it and stay small.'),
  tool('s3', 'byte', 57, 'Read src/pace.js', 'read'),
  { id: 's4', botId: 'byte', kind: 'tool', at: 57, data: { title: 'Edit src/pace.js', toolKind: 'edit', status: 'completed', author: 'byte', diffs: [{ path: 'src/pace.js', added: 10, removed: 0, isNew: false, startLine: 18, patch: weeklyPatch }] } },
  { id: 's5', botId: 'byte', kind: 'tool', at: 56, data: { title: 'npm test', toolKind: 'execute', command: 'npm test', status: 'completed', output: '✓ isoWeek: mid year\n✓ isoWeek: first week\n✓ weeklyTotals: sums one week\n✓ weeklyTotals: week that crosses into 2027\n\n4 passing (12ms)', author: 'byte' } },
  reply('s6', 'byte', 'byte', 55, 'I added `weeklyTotals(runs)` to `src/pace.js`. It takes runs shaped like `{ date, distance, time }`, groups them by ISO week in UTC (keys like `"2026-W53"`) and sums distance and time. The new test covers the week that crosses into 2027. All 4 tests pass and nothing is committed yet.'),

  // Crivo looks it over.
  user('r1', 'crivo', 50, 'Review weeklyTotals before we ship it.'),
  tool('r2', 'crivo', 49, 'Read src/pace.js', 'read'),
  tool('r3', 'crivo', 49, 'Read test/pace.test.js', 'read'),
  reply('r4', 'crivo', 'crivo', 48, 'I found no blockers. The ISO week math is right, including the New Year edge.\n\nTwo small things:\n- A bad date becomes a `"NaN-WNaN"` bucket instead of an error.\n- Weeks are grouped in UTC, so a late evening run west of UTC can land in the next week.'),

  // Prosa writes it up; a reply quotes its summary.
  user('l1', 'prosa', 41, 'Summarize what changed in pace today for the team channel.'),
  prosaSummary,
  user('l3', 'prosa', 38, 'Make it one line for the changelog.', prosaSummary),
  reply('l4', 'prosa', 'prosa', 38, 'Pace now shows your weekly distance and time totals.'),

  // Lançamento.
  user('g1', 'lancamento', 22, 'Is weeklyTotals ready to ship? One short sentence each.'),
  reply('g2', 'lancamento', 'crivo', 21, 'Almost: the week math is right and all 4 tests pass, but a bad date gets bucketed as `"NaN-WNaN"` instead of raising an error. @Byte, can you add an invalid-date guard?'),
  byteGuard,
  user('g4', 'lancamento', 17, 'UTC is right, keep it.', byteGuard),
  reply('g5', 'lancamento', 'byte', 17, 'Got it, UTC stays. I left a comment above `weeklyTotals` so nobody flips it later.'),
  reply('g6', 'lancamento', 'sprint', 15, 'Ship it once my `isoWeek` New Year fix lands. Dec 30, 2024 still comes out as 2024-W01 instead of 2025-W01.'),

  // Berga (the main bot, id sprint) waits for approval.
  user('p1', 'sprint', 9, 'Fix the isoWeek New Year bug you mentioned.'),
  thought('p2', 'sprint', 9, 'The year is read before moving to the Thursday of the week, so late December dates get the wrong ISO year.'),
  tool('p3', 'sprint', 8, 'Read src/pace.js', 'read'),
  { id: 'p4', botId: 'sprint', kind: 'agent', at: 7, data: { text: 'Found it: the year comes from the date itself, not from the Thursday of its week.', final: false, author: 'sprint' } },
  {
    id: 'p5', botId: 'sprint', kind: 'permission', at: 6,
    data: {
      title: 'Edit src/pace.js', toolKind: 'edit', cwd: CWD, status: 'pending', selected: undefined, author: 'sprint',
      diffs: [{ path: 'src/pace.js', added: 2, removed: 1, isNew: false, startLine: 4, patch: isoWeekPatch }],
      options: [
        { optionId: 'allow', name: 'Allow', kind: 'allow_once' },
        { optionId: 'always', name: 'Always allow', kind: 'allow_always' },
        { optionId: 'deny', name: 'Deny', kind: 'reject_once' },
      ],
    },
  },
]

/** Barra05: sessões além da principal (o catálogo do RPC `sessions`); `at` em minutos atrás; a última do dono em
 * "Invalid-date guard" deixa ela rodando, e o cartão pendente de "Changelog draft" faz ela pedir você. */
export interface SeedSession { chatId: string; id: string; title: string; at: number; closed?: boolean }
export const sessions: SeedSession[] = [
  { chatId: 'byte', id: 'byte-guard', title: 'Invalid-date guard', at: 3 },
  { chatId: 'byte', id: 'byte-docs', title: 'Changelog draft', at: 12 },
  { chatId: 'byte', id: 'byte-tz', title: 'Local time zones research', at: 35 },
  { chatId: 'byte', id: 'byte-sem-titulo', title: '', at: 50 },
  { chatId: 'byte', id: 'byte-bench', title: 'Benchmark weeklyTotals', at: 80 },
  { chatId: 'byte', id: 'byte-lint', title: 'Lint the pace module', at: 200 },
  { chatId: 'byte', id: 'byte-ci', title: 'CI cache for npm', at: 1500 },
  { chatId: 'byte', id: 'byte-old1', title: 'Rename helpers', at: 3000, closed: true },
  { chatId: 'byte', id: 'byte-old2', title: 'First weekly totals spike', at: 4400, closed: true },
  { chatId: 'lancamento', id: 'lancamento-notes', title: 'Release notes 1.4', at: 14 },
  { chatId: 'lancamento', id: 'lancamento-qa', title: 'QA checklist', at: 90 },
]
/** Entradas dentro das sessões (`sessionId`). */
export const sessionEntries: (SeedEntry & { sessionId: string })[] = [
  { ...user('ss1', 'byte', 12, 'Draft the changelog entry for weeklyTotals.'), sessionId: 'byte-docs' },
  { id: 'ss2', botId: 'byte', kind: 'permission', at: 11, sessionId: 'byte-docs', data: { title: 'Edit CHANGELOG.md', toolKind: 'edit', cwd: CWD, status: 'pending', author: 'byte', options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }, { optionId: 'deny', name: 'Deny', kind: 'reject_once' }] } },
  { ...user('ss5', 'byte', 30, 'Which zones do our runners use?'), sessionId: 'byte-tz' },
  { id: 'ss6', botId: 'byte', kind: 'user', at: 4, sessionId: 'byte-tz', data: { text: 'Also check Lisbon and São Paulo.', status: 'queued' } },
  { ...user('ss3', 'byte', 3, 'Add the invalid-date guard and a test for it.'), sessionId: 'byte-guard' },
  { ...reply('ss4', 'byte', 'byte', 2, 'Working on it: the guard goes at the top of `weeklyTotals`.'), sessionId: 'byte-guard' },
  // Proposta do painel (07/11): uma sessão com mensagem na fila, uma sem título (vale o 1º pedido) e uma com resposta.
  // O pedido mais novo vai para o benchmark: ele roda, e a escolha (`?escolha`) fica em Invalid-date guard.
  { ...user('ss10', 'byte', 1, 'Benchmark weeklyTotals with 10k runs.'), sessionId: 'byte-bench' },
  { ...user('ss7', 'byte', 50, 'Compare the pace module with last week\'s numbers.'), sessionId: 'byte-sem-titulo' },
  { ...reply('ss8', 'byte', 'byte', 49, 'Last week was 4% faster on the long run; the rest matches.'), sessionId: 'byte-sem-titulo' },
  { ...reply('ss9', 'byte', 'byte', 199, 'Lint is clean: two unused imports removed.'), sessionId: 'byte-lint' },
]
/** O líder de cada canal (contrato do Hub: `leader`). */
export const leaders: Record<string, string> = { lancamento: 'crivo' }

/** What a bot says back to a new message: rotated so repeated sends read differently. */
export const replies: Record<string, string[]> = {
  sprint: [
    'Sure. I\'ll keep it inside `src/pace.js` and rerun `npm test` when it\'s done.',
    'Makes sense. The week math is the risky part, so I\'ll add a test for it first.',
    'Good call. Nothing else imports that function, so it\'s a one-file change.',
  ],
  byte: [
    'On it. I\'ll grep the repo first so we know what else touches it.',
    'Done a quick pass: only `src/pace.js` and its test use it, so the change stays small.',
    'Sure. I\'ll write the test first, then make it pass.',
  ],
  crivo: [
    'I\'ll look at the diff once it lands and flag anything that isn\'t a nit.',
    'Looks fine to me. Just keep the test for the New Year week, that\'s where it broke before.',
    'One thing to watch: dates without a time zone are read as UTC. Worth a comment.',
  ],
  prosa: [
    'Here\'s a draft: "Pace now groups your runs by week, with totals for distance and time."',
    'Noted. I\'ll fold that into the release notes for the next version.',
    'Short version for the channel: weekly totals are in, a New Year week fix is next.',
  ],
}

export const waitingReply = 'I\'m still waiting on your approval for the `isoWeek` edit. Once you allow it, I\'ll keep going.'
export const approvedReply = 'Fixed: `isoWeek` now takes the year from the Thursday of the week, so Dec 30, 2024 is 2025-W01. I added a test for it and all 6 tests pass.'
export const deniedReply = 'OK, I left `src/pace.js` alone. Want me to explain the fix so you can make it yourself?'

export function usage(now: number): Usage {
  const hours = (h: number) => now + h * 3_600_000
  // MX-245, só `?uso=longo`: empurra as janelas semanais para além de 7 dias, a forma mais longa da frase de
  // retorno ("volta {data} às {hora}"), para a prova da coluna de Ajustes → Uso e da ilha nos 6 idiomas.
  const longo = typeof location !== 'undefined' && new URLSearchParams(location.search).get('uso') === 'longo'
  return {
    providers: [
      { id: 'claude', name: 'Claude', source: 'statusline', updatedAt: now, windows: [
        { id: 'five_hour', label: '5-hour', percent: 38, resetsAt: hours(2.4) },
        { id: 'seven_day', label: 'Weekly', percent: 61, resetsAt: hours(longo ? 24 * 9 : 70) },
        // `uso=longo`: a terceira janela (2×3, o pior caso real da ilha, D-245).
        ...(longo ? [{ id: 'opus', label: 'Weekly · Opus', percent: 74, resetsAt: hours(24 * 9.5) }] : []),
      ] },
      { id: 'codex', name: 'Codex', source: 'sessions', updatedAt: now, windows: [
        { id: 'primary', label: '5-hour', percent: 22, resetsAt: hours(3.1) },
        // `uso=longo`: texto literal do provedor (sem instante), sempre da esquerda para a direita e sem
        // tradução — o pior caso real de comprimento, mais longo que qualquer forma traduzida.
        longo ? { id: 'secondary', label: 'Weekly', percent: 47, resetsText: 'Sep 26 at 12:00pm (America/Argentina/Buenos_Aires)' }
          : { id: 'secondary', label: 'Weekly', percent: 47, resetsAt: hours(95) },
        ...(longo ? [{ id: 'opus', label: 'Weekly · Opus', percent: 88, resetsAt: hours(24 * 10.5) }] : []),
      ] },
    ],
  }
}
