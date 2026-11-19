import { useEffect } from 'react'
import { forkFeatures } from '@shared/fork-features'
import { ForkRelayTransport } from '../../../client/fork-relay-transport'
import { useModel } from '../../../lib/observable'
import { account } from '../../../store/account'
import type { AppModel } from '../../../store/app-model'
import { BotStore } from '../../../store/bot-store'
import { CloudAttachmentBinder } from './fork-cloud-attachments'
import { linkCaido, transporteParado } from '../../../client/fork-transporte-parado'
import { LinkTrocavel, LojaSegura, lojasJuntas, nuvemQuedaTemporaria } from '../../../store/fork-queda-curta'
import { deviceLabel, ForkCloudModel } from './fork-cloud-model'

let model: ForkCloudModel | null = null

/** O modelo da janela, criado na primeira vez; null enquanto `cloudAccess` estiver desligada (nada é lido). */
export function forkCloudModel(): ForkCloudModel | null {
  if (!forkFeatures.cloudAccess) return null
  return (model ??= new ForkCloudModel(window.codync.forkCloud, account, () =>
    deviceLabel(window.codync.computerName, window.codync.platform === 'darwin' ? 'Mac' : 'Computador')))
}

export function useForkCloud() {
  const cloud = forkCloudModel()
  useEffect(() => cloud?.start(), [cloud])
  return useModel(cloud)
}

/**
 * Anexa o Hub conectado pela nuvem como uma loja pela ponte (não loopback), no contexto de cache
 * da conta que o Main deu, enquanto o anexo qualificado existir. MX-217: numa queda curta do canal a loja fica
 * (store/fork-queda-curta.ts) e o canal novo ao mesmo Hub a retoma. Chame uma vez, do ChatWindow, depois do SSH.
 */
export function useForkCloudAttachments(app: AppModel) {
  useEffect(() => {
    const cloud = forkCloudModel()
    if (!cloud) return
    cloud.start()
    const links = new WeakMap<BotStore, LinkTrocavel<string>>()
    // A loja raiz de cada computador (não a do space aberto): é ela que o binder anexa e solta.
    const raiz = { store: (id: string) => app.stores.get(id) ?? null, attach: app.attach.bind(app), detach: app.detach.bind(app), subscribe: app.subscribe }
    const binder = new CloudAttachmentBinder<BotStore>(raiz, cloud, (a, computer) => {
      const link = new LinkTrocavel<string>(a.connectionId, (id) => new ForkRelayTransport(id), transporteParado, linkCaido)
      const store = new BotStore(computer, false, link.transporte, window.codync.appVersion, a.contextId)
      links.set(store, link)
      return store
    }, {
      segura: (store, computerId) => new LojaSegura(lojasJuntas(() => app.spaceMirrors.doComputador(computerId)), links.get(store)!),
      temporaria: (computerId) => nuvemQuedaTemporaria(cloud.state, computerId),
    })
    return binder.start()
  }, [app])
}
