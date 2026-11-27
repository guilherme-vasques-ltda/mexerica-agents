// Na Open, a aba Motor: as contas Codex conectadas e os modelos que elas comprovaram. A escolha vai ao Hub por
// `donnaOpenModelSet` (só dono local, só conta Open conectada, só modelo comprovado); a tela só mostra e envia.
import { useEffect, useState } from 'react'
import type { Bot } from '@shared/models'
import { Button, CardSection } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useSelector } from '../../../lib/observable'
import { useStoreRef } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts'
import { clientePessoal } from '../contas-ia/fork-open-cliente'
import { aoMudarContaOpen, aplicarModeloOpen, contasEscolhiveis, modelosDaConta, padraoDaConta } from '../contas-ia/fork-open-modelo'

export function MotorDaOpen({ bot, disabled }: { bot: Bot; disabled: boolean }) {
  const store = useStoreRef()
  useSelector(store, (s) => s.client)
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  const [contas, setContas] = useState<ManagedAccount[]>([])
  const [gravando, setGravando] = useState(false)
  const [erro, setErro] = useState(false)

  useEffect(() => {
    const client = store.client ? clientePessoal(store.client) : null
    if (!client) { setContas([]); return }
    let vivo = true
    const ler = () => { void engineSetup.accounts(client).then((r) => { if (vivo) setContas(contasEscolhiveis(r.accounts)) }, () => {}) }
    ler()
    const parar = aoMudarContaOpen(ler)
    return () => { vivo = false; parar() }
  }, [store, store.client])

  const escolher = async (accountId: string, model: string) => {
    if (!store.client) { setErro(true); return }
    setGravando(true)
    setErro(false)
    try { await aplicarModeloOpen(store.client, { accountId, model, botId: bot.id }) } catch { setErro(true) } finally { setGravando(false) }
  }

  const contaId = bot.forkEditor?.engineAccountId ?? null
  return <div className="idioma-superficie" {...idioma}>
    <p>{t('botSettings.open.motor.intro')}</p>
    {contas.length === 0 ? <p role="status">{t('botSettings.open.motor.semConta')}</p> : contas.map((conta) => <CardSection key={conta.id} title={conta.label}>
      {modelosDaConta(conta).map((model) => {
        const marcado = conta.id === contaId && model === bot.model
        const rotulo = model === padraoDaConta(conta) ? t('botSettings.open.motor.padrao', { modelo: model }) : model
        return <div key={model} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
          <span dir="ltr">{rotulo}</span>
          <Button kind={marcado ? 'primary' : 'secondary'} disabled={disabled || gravando || marcado} onClick={() => { void escolher(conta.id, model) }}>{marcado ? '✓' : t('botSettings.open.motor.usar')}</Button>
        </div>
      })}
    </CardSection>)}
    {erro ? <p role="alert">{t('botSettings.open.motor.erro')}</p> : null}
  </div>
}
