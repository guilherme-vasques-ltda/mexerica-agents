// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useRef, useState } from 'react'
import { rebase } from '@shared/draft-rebase'
import { draftOf, type Bot, type BotDraft } from '@shared/models'
import { randomAvatar } from '@shared/fork-avatar-config'
import { fillNewBotDefaults, NEW_BOT_BACKEND } from '@shared/new-bot-defaults'
import type { BotStore } from '../../store/bot-store'
import type { SavedLogin } from '../marketplace/market-models'
import { erroDaDonna } from '../../lib/fork-donna-erros.ts'

/** An installed connector (host `connectors`). */
export interface InstalledConnector {
  id: string
  name: string
  description: string
  kind: string
  command?: string | null
  url?: string | null
}

/** An installed skill (host `skills`); the Hub's identity and content state guard a choice (c1). */
export interface InstalledSkill {
  id: string
  name: string
  description: string
  installationId?: string
  contentStatus?: string
}

export interface Plugins {
  connectors: InstalledConnector[]
  skills: InstalledSkill[]
  /** Saved logins; each bot turns on the ones it may use (D82). */
  logins: SavedLogin[]
  /** D169: the Hub's own error for a list it couldn't load; an empty list is never shown in its place. */
  errors?: { connectors?: string; skills?: string; logins?: string }
  /** Loads the lists again (after an error). */
  retry?: () => void
}

/** A fresh draft: Codex, no connectors/accounts, Pedir until the general setting loads. */
export function newDraft(): BotDraft {
  return { name: '', description: '', avatarColor: 'blue', avatarShape: 'blob', avatarConfig: randomAvatar(), backend: NEW_BOT_BACKEND, cwd: '', permission: 'ask', connectors: [], logins: [] }
}

/** A saved bot keeps a name; only a new one may go without (the host names it after its first conversations). */
export const isValid = (d: BotDraft) => d.name.trim() !== ''

/** What the host should get: no stale command unless the agent is custom. */
export const normalized = (d: BotDraft): BotDraft => (d.backend === 'custom' ? d : { ...d, command: null })

/**
 * Picks an installed agent we know (a registry agent nobody chose is never the default);
 * empty cwd requests a personal workspace from the host. Connectors/accounts start empty;
 * skills keep the host's defaults.
 */
export function fillDefaults(store: BotStore, draft: BotDraft): BotDraft {
  return fillNewBotDefaults(draft, store.hello?.backends ?? [])
}

type PluginClient = NonNullable<BotStore['client']>
const listSkills = (client: PluginClient) => client.call<{ items: InstalledSkill[] }>('skills')

/** The connectors, skills and logins on the computer (kit `refreshPlugins`); `skills` may read more (fork R3). */
export async function loadPlugins(store: BotStore, skills: (client: PluginClient) => Promise<{ items: InstalledSkill[] }> = listSkills): Promise<Plugins> {
  const client = store.client
  if (!client) return { connectors: [], skills: [], logins: [] }
  const [c, s, l] = await Promise.allSettled([
    client.call<{ items: InstalledConnector[] }>('connectors'),
    skills(client),
    client.call<{ items: SavedLogin[] }>('credentialLogins'),
  ])
  const items = <T>(r: PromiseSettledResult<{ items: T[] }>) => (r.status === 'fulfilled' ? r.value.items : [])
  const error = (r: PromiseSettledResult<unknown>) => (r.status === 'rejected' ? errorText(r.reason) : undefined)
  const errors = { connectors: error(c), skills: error(s), logins: error(l) }
  return { connectors: items(c), skills: items(s), logins: items(l), ...(errors.connectors || errors.skills || errors.logins ? { errors } : {}) }
}

export function usePlugins(store: BotStore): Plugins | null {
  const [plugins, setPlugins] = useState<Plugins | null>(null)
  const [attempt, setAttempt] = useState(0)
  const online = store.connection.kind === 'online'
  useEffect(() => {
    let live = true
    void loadPlugins(store).then((p) => live && setPlugins({ ...p, retry: () => { setPlugins(null); setAttempt((n) => n + 1) } }))
    return () => {
      live = false
    }
  }, [store, online, attempt])
  return plugins
}

/**
 * Keeps an open settings form on the host's copy of its bot: a change made elsewhere (a bot that manages the team,
 * another device) replaces each field the user hasn't edited here, so saving the form never puts back the old value.
 */
export function useFollowSaved(bot: Bot | undefined, change: (update: (d: BotDraft) => BotDraft) => void) {
  const saved = bot ? JSON.stringify(draftOf(bot)) : null
  const last = useRef(saved)
  useEffect(() => {
    const old = last.current
    last.current = saved
    if (!old || !saved || old === saved) return
    const [base, fresh] = [JSON.parse(old) as BotDraft, JSON.parse(saved) as BotDraft]
    if (base.id === fresh.id) change((d) => rebase(d, base, fresh))
  }, [saved])
}

export const lastPathComponent = (path: string) => path.split('/').filter(Boolean).pop() ?? path

export const errorText = (e: unknown) => {
  const message = e instanceof Error ? e.message : String(e)
  return erroDaDonna(message) ?? message
}
