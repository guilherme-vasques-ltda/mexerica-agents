// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { spaceBotLabel } from '../lib/fork-spaces-rail'
import { avatarDoBot, botAccent } from '@shared/fork-avatar-config'
import { useEffect, useRef } from 'react'
import type { IslandBot, IslandPermission, IslandSummary, TurnSeen } from '@shared/island'
import { janelaVista, observeTurns } from '@shared/island'
import { folderName, isGroup, isWorking, type Bot, type Entry } from '@shared/models'
import { useModels } from '../lib/observable'
import { refKey, sameRef, type AppModel, type RosterItem } from '../store/app-model'
import type { BotStore } from '../store/bot-store'
import { botDetail } from './tray-summary'
import { optionLabel, permissionHeadline, sortedOptions } from './thread/PermissionCard'
import { permissionDetail } from './thread/permission-copy'
import { stepText } from './thread/cards/step-text'
import { idiomaAtual, tradutor } from '../lib/fork-i18n/estado'
import { settingsScreen } from './fork/ajustes/tela'
import { extensoesScreen } from './fork/extensoes/tela'
import { routinesScreen } from './rotinas-central/navigation'
import { timesScreen } from './fork/times/navigation'
import { torreScreen } from './fork/torre/navigation'

// Fatia 1 (D454): a ilha segue o idioma do app; o resumo é montado no idioma da hora em que sai.
const tIlha = () => tradutor(idiomaAtual())

/** Tool calls in the island's ticker. */
const STEPS = 3

let sent = ''
let pending: ReturnType<typeof setTimeout> | null = null
/** Por bot: âncora do turno em andamento e, congelados no fim, a duração e o "estou olhando". */
const turnSeen = new Map<string, TurnSeen>()

/**
 * Varredura no instante em que a mudança é observada na store (não nos 150 ms da agregação nem no
 * efeito do React): congela a duração do turno e o "estou olhando" da transição, antes de seleção
 * ou foco mudarem.
 *
 * `watching` é tri-estado, fail-safe: só `false` (fora da vista provado) permite o banner.
 * A leitura da janela passa por `janelaVista`: foco e `visibilityState` crus do DOM viram
 * `true`/`false`/`null`, e qualquer valor fora do conjunto conhecido (ausente, undefined,
 * estado não suportado, leitura que lança) é `null` = desconhecido — nunca "fora da vista".
 * Janela provadamente à vista sem seleção conhecida (`app.selection === null`) também é `null`,
 * e desconhecido não notifica. As telas que ocupam a janela (Ajustes, Extensões, Rotinas, Times,
 * Torre — o mesmo `covered` do ChatWindow) mantêm a seleção, então a conversa coberta conta como
 * fora da vista (`false`), não como à vista.
 *
 * A observação inteira é isolada: qualquer exceção é engolida com log, sem conteúdo do evento,
 * porque este aviso é opcional e a falha dele nunca pode propagar para o caminho que entrega
 * os eventos/linhas do stream (o `apply` da store também a isola — ver `onEventApplied`).
 */
const focoDom = () => { try { return document.hasFocus() } catch { return null } }
const visivelDom = () => { try { return document.visibilityState } catch { return null } }

function observe(app: AppModel) {
  try {
    const looking = janelaVista(focoDom(), visivelDom())
    const covered = settingsScreen.open || extensoesScreen.open || routinesScreen.open || timesScreen.open || torreScreen.open
    observeTurns(
      turnSeen,
      (app.allRoster ?? app.roster)
        .filter((i) => !isGroup(i.bot))
        .map((i) => ({
          ref: refKey(i.ref),
          status: i.bot.status,
          startedAt: i.bot.startedAt,
          watching: looking === false || covered ? false : looking !== true || app.selection === null ? null : sameRef(app.selection, i.ref),
        })),
    )
  } catch (error) {
    console.warn('ilha: observação de turno falhou', error)
  }
}

function scheduleSummary(app: AppModel) {
  observe(app)
  if (pending) return
  pending = setTimeout(() => {
    pending = null
    const summary = islandSummary(app)
    const json = JSON.stringify(summary)
    if (json === sent) return
    sent = json
    window.codync.island.setSummary(summary)
  }, 150)
}

/**
 * Sends the notch island what it shows when the stores change: at most every 150 ms while
 * replies stream (reading the stores when it fires), and only when that changed.
 */
export function useIslandSummary(app: AppModel) {
  const stores = (app.spaceMirrors?.all ?? [...app.stores.values()])
  const observed = useRef(stores)
  if (observed.current.length !== stores.length || stores.some((store, i) => store !== observed.current[i])) observed.current = stores
  const avatarStores = observed.current
  useModels(stores)
  useEffect(() => {
    const offs = avatarStores.flatMap((store) => {
      // MX-251: `changed()` agrega as notificações num único microtask — um lote com working+idle
      // chegaria ao assinante como só idle e o "Terminou" se perderia. `onEventApplied` observa
      // cada evento no ponto em que o bot-sync o aplica; a assinatura fica de resguardo para
      // mutações locais fora do stream. Limitações aceitas (fail-safe, só afetam o aviso opcional
      // "Terminou", nunca falha/aprovação): (1) eventos aplicados antes deste efeito — store nova
      // ainda não assinada — e colapsos dentro de uma única mutação podem perder um "Terminou";
      // (2) nas telas que cobrem a janela (Ajustes, Extensões, Rotinas, Times, Torre) a conversa
      // conta como fora da vista, então um turno longo terminado ali PODE abrir o banner.
      store.onEventApplied = () => observe(app)
      return [
        store.subscribe(() => observe(app)),
        store.avatarMoods.subscribe(() => scheduleSummary(app)),
        () => { store.onEventApplied = null },
      ]
    })
    return () => offs.forEach((off) => off())
  }, [app, avatarStores])
  useEffect(() => { scheduleSummary(app) })
}

function islandSummary(app: AppModel): IslandSummary {
  const many = app.computers.length > 1
  return {
    bots: (app.allRoster ?? app.roster)
      .filter((i) => !isGroup(i.bot))
      .map((i) => {
        const ref = refKey(i.ref)
        return islandBot(i.store, i.bot, ref, many ? `${spaceBotLabel(i.bot, i.store.spaces)} · ${i.computer.name}` : spaceBotLabel(i.bot, i.store.spaces), turnSeen.get(ref))
      }),
    permissions: (app.allRoster ?? app.roster).flatMap(pendingCards),
    usage: app.local?.usage.providers ?? [],
  }
}

function islandBot(store: BotStore, bot: Bot, ref: string, name: string, seen: TurnSeen | undefined): IslandBot {
  const entries = store.allEntries(bot.id)
  return {
    ref,
    name,
    avatarConfig: avatarDoBot(bot),
    avatarMood: store.avatarMoods.get(bot.id),
    avatarLabel: bot.name,
    shape: bot.avatarShape,
    color: botAccent(bot),
    status: bot.status,
    working: isWorking(bot),
    backend: store.backendName(bot.backend),
    folder: folderName(bot),
    detail: botDetail(tIlha(), bot, idiomaAtual()),
    steps: isWorking(bot)
      ? entries.filter((e) => e.kind === 'tool').slice(-STEPS).map((e) => {
          const kind = e.data.toolKind ?? 'other'
          return e.data.title
            ? { title: stepText(tIlha(), { title: e.data.title, kind }), original: e.data.title, kind }
            : { title: tIlha()('conversa.aprovacao.acao.ferramenta'), kind }
        })
      : [],
    error: bot.status === 'error' ? (lastError(entries) ?? null) : null,
    duracaoMs: seen?.duracaoMs ?? null,
    watching: seen?.watching ?? null,
  }
}

/** The text of the conversation's last error notice. */
const lastError = (entries: Entry[]) => entries.findLast((e) => e.kind === 'notice' && e.data.style === 'error')?.data.text

/** A conversation's unanswered approval cards; in a group, the member that asks. */
function pendingCards({ ref, bot, store }: RosterItem): IslandPermission[] {
  return store
    .allEntries(bot.id)
    .filter((e) => e.kind === 'permission' && e.data.status === 'pending' && e.data.toolKind !== 'open_local_command')
    .map((e) => {
      const author = (e.data.author ? store.bots.get(e.data.author) : undefined) ?? bot
      return {
        ref: refKey(ref),
        entryId: e.id,
        bot: { name: author.name, shape: author.avatarShape, color: botAccent(author), backend: store.backendName(author.backend), avatarConfig: avatarDoBot(author), avatarMood: store.avatarMoods.get(author.id), avatarLabel: author.name },
        headline: permissionHeadline(tIlha(), e.data.toolKind),
        title: permissionDetail(tIlha(), e.data.toolKind, e.data.title),
        options: sortedOptions(e.data.options ?? []).map((o) => ({ id: o.optionId, label: optionLabel(tIlha(), o, false, !!e.data.access), kind: o.kind })),
        answering: store.answering.get(e.id) ?? null,
      }
    })
}
