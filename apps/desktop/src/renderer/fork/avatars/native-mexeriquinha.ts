import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { conjuntoDoAvatar } from '@shared/fork-mexeriquinha-config'
import type { Bot } from '@shared/models'
import { avatarMoodsFor } from '@shared/fork-avatar-mood'
import { renderSimples } from '../../lib/mexeriquinhas/index.ts'
import { estadoDoBot, quadrosDoEstado } from '../../lib/mexeriquinhas-runtime.ts'
import { quadroPeluciaParaCanvas } from './mexeriquinhas-assets.ts'

const fontes = new Map<string, Promise<string | undefined>>()
const imagens = new Map<string, HTMLImageElement>()

/** A mesma escolha do app, rasterizada pelo canvas existente só para o menu nativo. */
export function imagemDaMexeriquinha(bot: Bot, aoCarregar: () => void): { key: string; imagem: HTMLImageElement | null } {
  const config = normalizarAvatarDoBot(avatarDoBot(bot), bot.name, bot.description)
  const moods = avatarMoodsFor(bot)
  const atual = moods?.available() === false ? null : estadoDoBot(bot, Date.now(), moods?.activity(bot.id))
  const estado = atual ?? 'livre' // Pose neutra, sem selo; não anuncia Livre quando o runtime é desconhecido.
  const quadro = quadrosDoEstado(estado, false)[0]!
  const key = `${JSON.stringify(config)}/${atual ?? 'unavailable'}`
  const hit = imagens.get(key)
  if (hit) return { key, imagem: hit.complete && hit.naturalWidth ? hit : null }
  if (!fontes.has(key)) {
    const fonte = config.estilo === 'simples'
      ? Promise.resolve(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(renderSimples(config.aparencia, { size: 40, state: estado, id: 'mxq-tray' }))}`)
      : quadroPeluciaParaCanvas(conjuntoDoAvatar(config)!, quadro)
    fontes.set(key, fonte)
  }
  void fontes.get(key)!.then((src) => {
    if (!src || imagens.has(key)) return
    const imagem = new Image()
    imagem.onload = aoCarregar
    imagem.decoding = 'async'
    imagem.src = src
    imagens.set(key, imagem)
    // Não manter toda a história das edições só para o menu de 20px.
    if (imagens.size > 64) {
      const oldest = imagens.keys().next().value!
      imagens.delete(oldest); fontes.delete(oldest)
    }
  }).catch(() => { fontes.delete(key) })
  return { key, imagem: null }
}
