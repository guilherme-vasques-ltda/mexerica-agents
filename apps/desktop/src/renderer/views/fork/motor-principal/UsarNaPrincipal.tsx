// DON-865, fatia 4: o aviso "Vale para conversas novas" (sempre) e, com o anúncio `engine` do Hub, o botão
// "Usar também na conversa principal" (`sessionEngineSet`, o mesmo da C9). Regras em `regras.ts`.
// Textos pelas chaves `botSettings.motor.*` dos catálogos (lib/fork-i18n).
import { useEffect, useState } from 'react'
import type { Bot } from '@shared/models'
import { Button } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { modeloDe } from '../contexto/BarraContexto'
import { usoDaConversa } from '../seletor-modelo/uso'
import { ConfirmarTroca } from '../seletor-modelo/ConfirmarTroca'
import { engineDaSessao, erroDaTroca, type Escolha } from '../seletor-modelo/regras'
import { confirmarNaPrincipal, estadoDoBotao, textoDaNota, usarNaPrincipal } from './regras'
import './motor-principal.css'

export function UsarNaPrincipal({ bot, accountId }: { bot: Bot; accountId: string | null }) {
  const store = useStore()
  const t = useT()
  const sessions = useModel(store.sessions)
  const [confirmar, setConfirmar] = useState(false)
  const [busy, setBusy] = useState(false)
  // `erro: null` = gravou; senão, a chave do catálogo do erro (a mesma da C9), traduzida só aqui na view.
  const [nota, setNota] = useState<{ erro: ReturnType<typeof erroDaTroca> | null } | null>(null)
  useEffect(() => { if (!sessions.of(bot.id)) void sessions.load(bot.id) }, [bot.id]) // eslint-disable-line react-hooks/exhaustive-deps

  const principal = engineDaSessao(sessions.of(bot.id)?.find((s) => s.principal) ?? null)
  // Fonte canônica do anúncio (`sessions.can`, por link/época, a mesma do chip da C9): sem `engine` anunciado, Hub
  // antigo ou leitura com erro, o botão some e nada chama o Hub.
  const anunciado = !!principal && sessions.can(bot.id, 'engine')
  const padrao: Escolha = { accountId, backend: bot.backend, model: bot.model ?? null, modelOptions: bot.modelOptions ?? {}, fastMode: bot.fastMode === true }
  const estado = estadoDoBotao(anunciado, principal, padrao)

  const gravar = async () => {
    if (!principal) return
    setBusy(true)
    setNota(null)
    try {
      const client = store.client
      if (!client) throw new Error('offline')
      await usarNaPrincipal(client, anunciado, bot.id, principal, padrao)
      setNota({ erro: null })
    } catch (error) {
      setNota({ erro: erroDaTroca(error) })
    } finally { setBusy(false) }
  }

  return <div className="motor-principal">
    <p className="motor-principal-aviso">{t('botSettings.motor.aviso')}</p>
    {estado === 'oculto' ? null : <Button kind="secondary" disabled={busy || estado === 'igual'} title={estado === 'igual' ? t('botSettings.motor.igual') : undefined}
      onClick={() => {
        if (!principal) return
        if (confirmarNaPrincipal(principal, padrao, usoDaConversa(modeloDe(store), bot.id, null))) setConfirmar(true)
        else void gravar()
      }}>{t('botSettings.motor.usar')}</Button>}
    {nota ? <p className={`motor-principal-nota${nota.erro ? ' erro' : ''}`} role={nota.erro ? 'alert' : 'status'}>{textoDaNota(t, nota.erro)}</p> : null}
    <ConfirmarTroca aberto={confirmar} onCancelar={() => setConfirmar(false)} onTrocar={() => { setConfirmar(false); void gravar() }} />
  </div>
}
