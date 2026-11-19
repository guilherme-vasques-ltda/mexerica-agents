import { useState } from 'react'
import type { Bot } from '@shared/models'
import { VStack } from '../../../components/Stack'
import { Button, RowLabel } from '../../../components/Controls'
import { useT, useIdioma, superficieIdioma } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { SwitchRow } from '../../bots/parts'

/** D510: estado confirmado pelo Hub; só a intenção do dono passa pela RPC dedicada. */
export function TelaPorComputador({ bot, disabled }: { bot: Bot; disabled: boolean }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const change = async (target: string, enabled: boolean) => {
    setBusy(true)
    setError(null)
    try {
      await (await store.ready()).call('donnaScreenAccess', { botId: bot.id, target, enabled,
        hubId: store.hello?.hostId, spaceId: bot.spaceId ?? '00000000-0000-4000-8000-000000000001' })
    } catch (e) { setError(e instanceof Error ? e.message : String(e)) }
    finally { setBusy(false) }
  }
  return <VStack gap={8} {...superficieIdioma(idioma)}>
    {(bot.screenAccess ?? []).map((row) => <VStack key={row.target} gap={4}>
      {row.available
        ? <SwitchRow title={t('ajustes.telaPorComputador.titulo')} detail={row.name} on={row.enabled}
            disabled={disabled || busy} onChange={(on) => { void change(row.target, on) }} />
        : <RowLabel label={row.name} detail={t('ajustes.telaPorComputador.indisponivel')} />}
      {row.available ? <p className="approval-note">{t('ajustes.telaPorComputador.aviso')}</p> : null}
      {row.available ? <p className="approval-note">{t('ajustes.telaPorComputador.emVoo')}</p> : null}
      {row.available && row.canEndTemporary ? <>
        <p className="approval-note">{t('conversa.aprovacao.tela.agoraNesteTurno')}</p>
        <Button kind="secondary" disabled={disabled || busy} onClick={() => { void change(row.target, false) }}>{t('conversa.aprovacao.tela.encerrarAgora')}</Button>
      </> : null}
    </VStack>)}
    {!bot.screenAccess ? <p className="approval-note">{t('ajustes.telaPorComputador.atualize')}</p> : null}
    {error ? <p className="bot-settings-control-error" role="alert">{error}</p> : null}
  </VStack>
}
