import { useState } from 'react'
import type { BotEditor } from '../../../../shared/fork-bot-editor'
import type { Engine } from '../../../../shared/fork-engine-accounts'
import { CardForm } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { useStore } from '../../../store/context'
import { ContasDoHub } from './ContasDoHub'
import { errorMessage } from './regras'
import { superficieIdioma, tradutor, useIdioma } from '../../../lib/fork-i18n/estado'

/** O atalho de entrar pela nuvem usa o mesmo setup assíncrono das contas. */
export function RemoteEngineSheet({ engine, botId }: { engine: Engine; botId?: string }) {
  const store = useStore()
  const [message, setMessage] = useState<'contas.remotoEscolhida' | null>(null)
  const [error, setError] = useState<unknown>(null)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const bot = botId ? store.bots.get(botId) : null
  const useAccount = async (accountId: string) => {
    const client = await store.ready()
    setError(null)
    try {
      const editor = await client.call<BotEditor>('donnaBotEditor', { botId })
      if (store.client !== client || !store.canManageEngineAccounts) return
      await client.call('donnaBotEditorSave', { botId, expectedVersion: editor.settingsVersion,
        patch: { backend: engine, model: null, modelOptions: {}, forkEditor: { engineAccountId: accountId } } })
      if (store.client === client) setMessage('contas.remotoEscolhida')
    } catch (e) { if (store.client === client) setError(e) }
  }
  return <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
    <ModalHeader icon="key" title={t('contas.pagina')} closeTitle={t('contas.fechar')} />
    <CardForm>
      <ContasDoHub forBot={bot ? { name: bot.name, engine, use: (id) => void useAccount(id) } : undefined} />
      {!bot ? <Text tone="secondary">{t('contas.remotoDica')}</Text> : null}
      {message ? <Text role="status">{t(message)}</Text> : null}
      {error ? <Text tone="danger" role="alert">{errorMessage(error, idioma)}</Text> : null}
    </CardForm>
  </div></SheetPage>
}
