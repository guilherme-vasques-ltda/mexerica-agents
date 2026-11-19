import { spaceBotLabel } from '../lib/fork-spaces-rail'
import { useEffect, useState } from 'react'
import type { TraySummary } from '@shared/ipc'
import { connectionStatus } from '@shared/ssh-startup'
import { isGroup, isUnavailable, isWorking, needsInput, folderName, screenUseSubtitle, UNAVAILABLE_TEXT, type Bot, type UsageWindow } from '@shared/models'
import { drawAvatarIcon } from '../fork/avatars/native-icon'
import { imagemDaMexeriquinha } from '../fork/avatars/native-mexeriquinha'
import { normalizedAvatar } from '@shared/fork-avatar-config'
import { proximoSono } from '../lib/mexeriquinhas-runtime'
import { useModels } from '../lib/observable'
import { prefs, usePref } from '../lib/prefs'
import { isSystemDark as isDark, palette } from '../lib/theme'
import type { AppModel } from '../store/app-model'
import { providerTint, resetDate, untilText, windowTitle } from './usage/usage-model'
import { mascotDataURL } from './usage/ProviderMascot'
import { useSSH } from './settings/ssh-model'
import { useIdioma, useT, type Tradutor } from '../lib/fork-i18n/estado'
import type { AppLanguage } from '@shared/fork-idioma'
import { activityText } from '../lib/fork-atividade'

const isMac = window.codync.platform === 'darwin'

/** Rendered icons by what they show and the appearance: nothing is redrawn while nothing changed. */
const cache = new Map<string, string>()

function canvasURL(key: string, width: number, height: number, draw: (ctx: CanvasRenderingContext2D) => void) {
  const full = `${key}|${isDark()}`
  const hit = cache.get(full)
  if (hit) return hit
  const ratio = 2
  const canvas = document.createElement('canvas')
  canvas.width = width * ratio
  canvas.height = height * ratio
  const ctx = canvas.getContext('2d')!
  ctx.scale(ratio, ratio)
  draw(ctx)
  const url = canvas.toDataURL('image/png')
  cache.set(full, url)
  return url
}

function avatarIcon(bot: Bot, aoCarregar: () => void) {
  const { key: aparencia, imagem } = imagemDaMexeriquinha(bot, aoCarregar)
  const key = `bot|${bot.id}|${aparencia}|${bot.status}|${bot.unread > 0}|${!!imagem}`
  return canvasURL(key, 20, 20, (ctx) => {
    const dark = isDark()
    if (imagem) {
      ctx.save()
      if (isUnavailable(bot)) ctx.globalAlpha = 0.45
      ctx.drawImage(imagem, 0, 0, 20, 20)
      ctx.restore()
    }
    else drawAvatarIcon(ctx, normalizedAvatar(bot.avatarConfig, bot.name, bot.description), 20)
    const badge = needsInput(bot) ? palette(dark).warning : bot.unread > 0 ? (dark ? '#FFFFFF' : '#000000') : null
    if (badge) {
      const r = needsInput(bot) ? 3.6 : 2.8
      ctx.beginPath()
      ctx.arc(20 - r, 20 - r, r + 1, 0, Math.PI * 2)
      ctx.fillStyle = dark ? '#1e1e1e' : '#f2f2f2'
      ctx.fill()
      ctx.beginPath()
      ctx.arc(20 - r, 20 - r, r, 0, Math.PI * 2)
      ctx.fillStyle = badge
      ctx.fill()
    }
  })
}

/** A bot's line under its name: that it needs you, what it's doing, or its last message. */
export function botDetail(t: Tradutor, bot: Bot, idioma: AppLanguage) {
  if (isUnavailable(bot)) return UNAVAILABLE_TEXT
  return needsInput(bot) ? t('sistema.barra.precisaResposta') : isWorking(bot) ? activityText(t, idioma, bot) || t('sistema.barra.trabalhandoAgora') : (bot.lastMessage ?? folderName(bot))
}

/** One usage limit's bar in the provider's color: amber from 70%, red from 90%. */
function barIcon(providerId: string, window: UsageWindow) {
  const percent = Math.round(window.percent)
  return canvasURL(`bar|${providerId}|${percent}`, 60, 16, (ctx) => {
    const dark = isDark()
    const color = window.percent >= 90 ? palette(dark).danger : window.percent >= 70 ? palette(dark).warning : providerTint(providerId)
    ctx.fillStyle = dark ? 'rgba(255,255,255,0.12)' : 'rgba(0,0,0,0.12)'
    ctx.beginPath()
    ctx.roundRect(0, 5.5, 60, 5, 2.5)
    ctx.fill()
    if (window.percent > 0) {
      ctx.fillStyle = color
      ctx.beginPath()
      ctx.roundRect(0, 5.5, Math.max(5, 60 * Math.min(1, window.percent / 100)), 5, 2.5)
      ctx.fill()
    }
  })
}

function usageLine(t: Tradutor, w: UsageWindow) {
  const reset = resetDate(w)
  const base = { titulo: windowTitle(t, w), pct: Math.round(w.percent) }
  return reset ? t('sistema.barra.uso.renova', { ...base, prazo: untilText(reset) }) : t('sistema.barra.uso.linha', base)
}

/** O último resumo enviado à bandeja: igual ao novo, nem IPC sai (o main também confere antes de refazer o menu). */
let lastSent: string | null = null

/** Sends what the menu bar shows whenever the stores change. */
export function useTraySummary(app: AppModel) {
  const stores = (app.spaceMirrors?.all ?? [...app.stores.values()])
  useModels(stores)
  const ssh = useSSH()
  const [textSize] = usePref(prefs.textSize)
  const [iconStyle] = usePref(prefs.usageIconStyle)
  const [, setArtes] = useState(0)
  // D454 (fatia 10b): o resumo sai no idioma do app; trocar o idioma redesenha e manda de novo.
  const t = useT()
  const idioma = useIdioma()
  const arteChegou = () => setArtes((n) => n + 1)
  const now = Date.now()
  const sleeps = (app.allRoster ?? app.roster).filter((i) => !isGroup(i.bot) && i.store.avatarMoods.available())
    .map((i) => proximoSono(i.bot, now, i.store.avatarMoods.activity(i.bot.id))).filter((ms): ms is number => ms !== null)
  const nextSleep = sleeps.length ? Math.min(...sleeps) : null
  useEffect(() => {
    if (nextSleep === null) return
    const timer = setTimeout(arteChegou, Math.min(nextSleep, 2_147_483_647))
    return () => clearTimeout(timer)
  }, [nextSleep])
  useEffect(() => {
    const timer = setTimeout(() => {
      const local = app.local
      const roster = (app.allRoster ?? app.roster).filter((i) => !isGroup(i.bot))
      const status = app.approvals.length
        ? t('sistema.barra.pedidoAcesso')
        : app.needsAttention
          ? t('sistema.barra.botPrecisa')
          : app.working > 0
            ? t.plural('sistema.barra.trabalhando', app.working)
            : connectionStatus(stores.some((s) => s.connection.kind === 'online'), ssh.state, idioma)
      const screen = local?.screen
      const summary: TraySummary = {
        status,
        canPair: !!local,
        approval: app.approvals[0]?.request.deviceName ?? null,
        bots: roster.map((i) => ({
          ref: `${i.ref.computerId}/${i.ref.botId}`,
          name: app.computers.length > 1 ? `${spaceBotLabel(i.bot, i.store.spaces)} · ${i.computer.name}` : spaceBotLabel(i.bot, i.store.spaces),
          detail: botDetail(t, i.bot, idioma),
          working: isWorking(i.bot),
          avatar: avatarIcon(i.bot, arteChegou),
        })),
        usage: (local?.usage.providers ?? []).map((p) => ({
          name: p.name,
          icon: mascotDataURL(p, 16, iconStyle, isDark()),
          lines: p.windows.map((w) => ({ title: usageLine(t, w), bar: barIcon(p.id, w) })),
        })),
        screen: screen
          ? {
              enabled: screen.enabled,
              subtitle: screenSubtitle(t, app),
              error: app.screenError,
            }
          : null,
        version: local?.hello?.version ?? null,
        needsAttention: app.needsAttention,
        textSize,
        usageIconStyle: iconStyle,
      }
      const json = JSON.stringify(summary)
      if (json === lastSent) return
      lastSent = json
      window.codync.app.setTraySummary(summary)
    }, 150)
    return () => clearTimeout(timer)
  })
}

function screenSubtitle(t: Tradutor, app: AppModel) {
  const local = app.local
  const screen = local?.screen
  if (!local || !screen || !screen.enabled) return t(isMac ? 'sistema.barra.tela.controleMac' : 'sistema.barra.tela.controleComputador')
  if (app.screenNeedsApproval) return t('sistema.barra.tela.aprovacao')
  if (!screen.connected) return t('sistema.barra.tela.iniciando')
  if (!screen.capture || !screen.input) return t('sistema.barra.tela.permissao')
  // R1: o nome só com uso comprovado neste Space; indisponível (ou bot fora desta lista) vem antes de contagem e Ready.
  return screenUseSubtitle(screen, (id) => local.bots.get(id)?.name)
}
