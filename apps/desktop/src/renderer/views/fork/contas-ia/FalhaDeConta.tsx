import { useEffect, useMemo, useState } from 'react'
import type { AccountUsage } from '../../../../shared/fork-engine-accounts'
import type { Bot } from '../../../../shared/models'
import { Button } from '../../../components/Controls'
import { superficieIdioma, tradutor, useIdioma } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useStore, useStoreRef } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import { settingsScreen } from '../ajustes/tela'
import { useOpenBotSettings } from '../bot-settings/bot-settings-entry'
import { engineDaSessao } from '../seletor-modelo/regras'
import { contasFor } from './contas-store'
import { reservaFor } from './reserva-store'
import { isEngine } from './regras'
import { fraseDaFalha, linhaDaConta, resumoUso, type BotaoFalha, type ContaFalha, type FalhaReason } from './falha-de-conta'
import './falha-de-conta.css'

/** A lista de contas que o app já tem; lê uma vez (e só quando alguém a pede), nunca uma chamada por linha. */
export function useContasDaFalha(ativo = true) {
  const store = useStoreRef()
  const contas = useModel(contasFor(store, () => store.ready(), engineSetup))
  useEffect(() => {
    // `loading` é lido no objeto vivo: a primeira linha que pede já marca, e as seguintes não repetem.
    const vivo = contasFor(store, () => store.ready(), engineSetup)
    if (ativo && !vivo.accounts && !vivo.loading && !vivo.error) void vivo.load()
  }, [ativo, store, contas.accounts, contas.error])
  return { contas: contas.accounts, falhou: !!contas.error && !contas.accounts }
}

/** O que cada botão faz: Contas de IA, ou a configuração do bot na aba Motor / Comportamento (pasta). */
export function useAbrirDaFalha(botId: string | null, accountId?: string) {
  const store = useStoreRef()
  const abrirBot = useOpenBotSettings()
  return (botao: BotaoFalha) => {
    // MEX-1391/F6-B: permitir o uso extra da conta parada; o Hub só aceita enquanto ela descansa e vale até o reset.
    if (botao.acao === 'uso-extra') {
      if (accountId) void reservaFor(store, () => store.ready(), engineSetup).act('allowExtra', accountId, store.spaceId).catch(() => settingsScreen.show('ai-accounts', 'contas'))
      else settingsScreen.show('ai-accounts', 'contas')
      return
    }
    if (botao.acao !== 'contas' && botId && abrirBot) abrirBot(store, botId, botao.acao === 'bot-pasta' ? 'behaviour' : 'engine')
    else settingsScreen.show('ai-accounts', 'contas')
  }
}

/** O texto do aviso vermelho quando o Hub mandou `reason`: o que houve, que conta o bot usa, quais estão conectadas e o botão. */
export function AvisoDeFalhaDeConta({ reason, botId, botName }: { reason: FalhaReason; botId: string | null; botName: string }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const { contas } = useContasDaFalha()
  const abrir = useAbrirDaFalha(botId, reason.accountId)
  const frase = fraseDaFalha(reason, { t, idioma, bot: botName, contas })
  return (
    <div className="falha-conta selectable" {...superficieIdioma(idioma)}>
      <span>{frase.texto}</span>
      {frase.contaDoBot ? <span className="falha-conta-info">{frase.contaDoBot}</span> : null}
      <span className="falha-conta-info">{frase.contasConectadas}</span>
      <div className="falha-conta-botoes">
        {frase.botoes.map((b, i) => <Button key={b.acao + b.rotulo} kind={i === 0 ? 'primary' : 'secondary'} onClick={() => abrir(b)}>{b.rotulo}</Button>)}
      </div>
    </div>
  )
}

/** Hub antigo (sem `reason`): o texto de sempre e só o caminho até as contas. */
export function BotaoAbrirContas({ label }: { label?: string }) {
  const t = tradutor(useIdioma())
  return <Button kind="secondary" onClick={() => settingsScreen.show('ai-accounts', 'contas')}>{label ?? t('conversa.avisos.falha.acao.abrirContas')}</Button>
}

/** "Não foi enviada" com `reason`: o botão certo logo ao lado; a frase inteira fica na dica e no aviso vermelho. */
export function AcaoDaMensagemFalha({ reason, botId, botName }: { reason: FalhaReason; botId: string | null; botName: string }) {
  const idioma = useIdioma()
  const { contas } = useContasDaFalha()
  const abrir = useAbrirDaFalha(botId, reason.accountId)
  const frase = fraseDaFalha(reason, { t: tradutor(idioma), idioma, bot: botName, contas })
  const principal = frase.botoes[0]!
  return <button className="falha-conta-link" title={frase.texto} onClick={() => abrir(principal)}>{principal.rotulo}</button>
}

/** A linha "Conta: Empresa · Claude Code · desconectada" (clicável: abre Contas de IA). */
export function LinhaDaConta({ accountId, backend, botId, onClick, className = '', contasDoEditor }: { accountId: string | null; backend: string | null; botId?: string; onClick?: () => void; className?: string; contasDoEditor?: readonly ContaFalha[] }) {
  const store = useStore()
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const lida = useContasDaFalha(!!accountId && !contasDoEditor)
  const contas = contasDoEditor ?? lida.contas
  const falhou = !contasDoEditor && lida.falhou
  const estado = contas?.find((c) => c.id === accountId)?.state
  const [uso, setUso] = useState<AccountUsage | null>(null)
  // MEX-1294/F6-A: o uso da conta no Space aberto. Mesma regra dos Detalhes (só com a capacidade de contas de IA que o
  // Hub deu a este aparelho) e a mesma leitura guardada: trocar de conversa não sobe um processo do CLI por linha.
  const pode = store.canManageEngineAccounts
  useEffect(() => {
    setUso(null)
    if (!accountId || estado !== 'connected' || !pode) return
    let live = true
    contasFor(store, () => store.ready(), engineSetup).usage(accountId, store.spaceId).then(
      (u) => { if (live) setUso(u) },
      () => { if (live) setUso(null) },
    )
    return () => { live = false }
  }, [accountId, estado, pode, store, store.spaceId])
  const usoResumo = useMemo(() => resumoUso(uso, t, idioma), [uso, t, idioma])
  const linha = linhaDaConta({ t, contas, accountId, backend, falhou, usoResumo, open: window.codync.appScheme === 'mexerica-open' })
  if (!linha) return null
  const classe = `conta-linha${linha.ok ? (linha.perto ? ' limite' : '') : ' atencao'} ${className}`.trim()
  const abrir = onClick ?? (() => settingsScreen.show('ai-accounts', 'contas'))
  return (
    <button className={classe} data-bot={botId} title={usoResumo && linha.ok ? `${linha.texto}\n${linha.dica ?? t('conversa.avisos.falha.cabecalho.dica')}` : linha.dica ?? t('conversa.avisos.falha.cabecalho.dica')} onClick={abrir} {...superficieIdioma(idioma)}>
      {linha.uso ? <><span className="conta-base">{linha.base}</span><span className="conta-uso">{`· ${linha.uso}`}</span></> : <span>{linha.texto}</span>}
    </button>
  )
}

/**
 * O cabeçalho da conversa: qual conta o bot usa e em que estado. A conta da conversa (`session.engine`, o que vale de
 * verdade no Pessoal/Berga) vence a do perfil do bot (`forkEditor`); bot com comando próprio não tem conta.
 */
export function ContaDaConversa({ bot, sessionId }: { bot: Bot; sessionId: string | null }) {
  const store = useStoreRef()
  const sessions = useModel(store.sessions)
  const session = sessions.of(bot.id)?.find((s) => (sessionId ? s.id === sessionId : s.principal)) ?? null
  const engine = engineDaSessao(session)
  const backend = engine?.backend ?? bot.backend
  if (!isEngine(backend)) return null
  return <LinhaDaConta accountId={engine ? engine.accountId ?? null : bot.forkEditor?.engineAccountId ?? null} backend={backend} botId={bot.id} />
}
