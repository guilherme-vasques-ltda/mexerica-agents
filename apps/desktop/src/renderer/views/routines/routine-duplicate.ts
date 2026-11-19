import type { Routine } from './routine'

export const canDuplicate = (routine: Routine | null, offline: boolean): boolean => routine !== null && !offline

export const awaitingRelease = (routine: Pick<Routine, 'enabled' | 'externalReleasePending'>): boolean =>
  !routine.enabled && routine.externalReleasePending === true

export interface EditorFields {
  name: string
  instruction: string
  timeout: string
  scheduleKey: string
}

export const hasPendingEdit = (routine: Routine, now: EditorFields, savedScheduleKey: string | null): boolean =>
  savedScheduleKey === null ||
  now.scheduleKey !== savedScheduleKey ||
  now.name !== routine.name ||
  now.instruction !== (routine.instruction ?? '') ||
  now.timeout !== String(routine.timeoutSeconds ?? 3600)

export interface DuplicateClient {
  call<T>(method: string, params: Record<string, unknown>): Promise<T>
}

export async function duplicateRoutine(client: DuplicateClient, botId: string, id: string): Promise<Routine> {
  const result = await client.call<{ routine: Routine }>('duplicateRoutine', { botId, id })
  return result.routine
}
