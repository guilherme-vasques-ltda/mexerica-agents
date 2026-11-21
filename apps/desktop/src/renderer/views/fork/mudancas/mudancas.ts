import type { Entry, FileDiff } from '../../../../shared/models.ts'

export const DIFF_LINE_LIMIT = 80

export interface DiffHunk {
  entryId: string
  stepIndex: number
  toolTitle: string
  toolKind: string
  diff: FileDiff
  truncated: boolean
  lineCount: number
}

export interface FileChangeGroup {
  path: string
  fileName: string
  isNew: boolean
  approxAdded: number
  approxRemoved: number
  hasTruncated: boolean
  hunks: DiffHunk[]
}

export interface ChangeGaps {
  partial: boolean
  untrackedCommandCount: number
  historyIncomplete: boolean
}

export interface TurnChanges {
  replyId: string
  turn: number
  sessionId: string
  authorId: string
  files: FileChangeGroup[]
  totalSteps: number
  approxAdded: number
  approxRemoved: number
  gaps: ChangeGaps
}

const UNTRACKED_KINDS = new Set(['execute', 'edit', 'delete', 'move'])

const sessionOf = (e: Entry) => e.sessionId ?? e.botId
const authorOf = (e: Entry) => e.data.author ?? e.botId
const scopeKey = (turn: number, sessionId: string, authorId: string) => `${turn}|${sessionId}|${authorId}`
const fileNameOf = (path: string) => path.split('/').filter(Boolean).pop() ?? path

export function shouldShowChangesTrigger(changes: TurnChanges): boolean {
  return changes.files.length > 0 || changes.gaps.untrackedCommandCount > 0 || changes.gaps.historyIncomplete
}

export function changesByReply(entries: readonly Entry[], hasOlderHistory = false): Map<string, TurnChanges> {
  const minTurnBySession = new Map<string, number>()
  const userTurns = new Set<string>()
  const toolsByScope = new Map<string, { entry: Entry; stepIndex: number }[]>()
  const lastReply = new Map<string, Entry>()
  const lastAgentAt = new Map<string, number>()
  const cutAt = new Map<string, number>()
  const scopeMeta = new Map<string, { turn: number; sessionId: string; authorId: string }>()

  for (const e of entries) {
    const sessionId = sessionOf(e)
    if (e.turn > 0) {
      const prev = minTurnBySession.get(sessionId)
      if (prev === undefined || e.turn < prev) minTurnBySession.set(sessionId, e.turn)
    }
    if (e.kind === 'user') {
      userTurns.add(`${e.turn}|${sessionId}`)
      continue
    }
    if (e.kind === 'permission') continue

    const authorId = authorOf(e)
    const k = scopeKey(e.turn, sessionId, authorId)
    if (e.kind === 'tool') {
      if (!scopeMeta.has(k)) scopeMeta.set(k, { turn: e.turn, sessionId, authorId })
      const list = toolsByScope.get(k) ?? []
      list.push({ entry: e, stepIndex: list.length + 1 })
      toolsByScope.set(k, list)
      if (e.data.interrupted === true) cutAt.set(k, Math.max(cutAt.get(k) ?? 0, e.createdAt))
    } else if (e.kind === 'agent') {
      if (!scopeMeta.has(k)) scopeMeta.set(k, { turn: e.turn, sessionId, authorId })
      lastAgentAt.set(k, e.createdAt)
      if (e.data.final === true || e.data.interrupted === true) lastReply.set(k, e)
    }
  }

  const out = new Map<string, TurnChanges>()
  for (const [k, reply] of lastReply) {
    const list = toolsByScope.get(k) ?? []
    if (list.length > 0) {
      const lastToolAt = list[list.length - 1]!.entry.createdAt
      const cut = reply.data.interrupted === true
        && (lastAgentAt.get(k) ?? 0) <= reply.createdAt
        && !list.some((s) => s.entry.createdAt > Math.max(reply.createdAt, cutAt.get(k) ?? 0))
      if (!(reply.createdAt >= lastToolAt || cut)) continue
    }

    const meta = scopeMeta.get(k)!
    const fileMap = new Map<string, FileChangeGroup>()
    let approxAdded = 0
    let approxRemoved = 0
    let partial = false
    let untrackedCommandCount = 0

    for (const { entry: te, stepIndex } of list) {
      const diffs = te.data.diffs
      if (diffs && diffs.length > 0) {
        for (const diff of diffs) {
          const patch = diff.patch ?? ''
          const lineCount = patch ? patch.split('\n').length : 0
          const truncated = patch.endsWith('…') || lineCount > DIFF_LINE_LIMIT
          if (truncated) partial = true
          const hunk: DiffHunk = {
            entryId: te.id,
            stepIndex,
            toolTitle: te.data.title ?? '',
            toolKind: te.data.toolKind ?? 'other',
            diff,
            truncated,
            lineCount,
          }
          approxAdded += diff.added
          approxRemoved += diff.removed
          let group = fileMap.get(diff.path)
          if (!group) {
            group = {
              path: diff.path,
              fileName: fileNameOf(diff.path),
              isNew: diff.isNew,
              approxAdded: 0,
              approxRemoved: 0,
              hasTruncated: false,
              hunks: [],
            }
            fileMap.set(diff.path, group)
          } else if (diff.isNew) {
            group.isNew = true
          }
          group.approxAdded += diff.added
          group.approxRemoved += diff.removed
          if (truncated) group.hasTruncated = true
          group.hunks.push(hunk)
        }
      } else if (UNTRACKED_KINDS.has(te.data.toolKind ?? '')) {
        untrackedCommandCount += 1
      }
    }

    const minTurn = minTurnBySession.get(meta.sessionId)
    const historyIncomplete = hasOlderHistory === true
      && meta.turn > 0
      && minTurn !== undefined
      && meta.turn === minTurn
      && !userTurns.has(`${meta.turn}|${meta.sessionId}`)

    const changes: TurnChanges = {
      replyId: reply.id,
      turn: meta.turn,
      sessionId: meta.sessionId,
      authorId: meta.authorId,
      files: [...fileMap.values()],
      totalSteps: list.length,
      approxAdded,
      approxRemoved,
      gaps: {
        partial,
        untrackedCommandCount,
        historyIncomplete,
      },
    }
    if (shouldShowChangesTrigger(changes)) out.set(reply.id, changes)
  }

  return out
}
