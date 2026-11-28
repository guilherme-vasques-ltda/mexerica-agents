// Aba "Time" do painel do bot: o que o bot pediu a colegas (`donnaTeamRequests`), com quem pegou, o que foi pedido e
// como está. Só lê; Abrir vai à conversa do colega. Erro de leitura é erro, nunca lista vazia.
import { useEffect, useRef, useState } from 'react'
import type { Bot } from '@shared/models'
import { isolado } from '@shared/bidi'
import { ago } from '@shared/fork-torre-pedidos'
import { parseTeamRequests, teamRequestLabel, type TeamRequestLabel, type TeamRequests } from '@shared/fork-time-pedidos'
import { BotAvatar } from '../../../components/Avatar'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n'
import type { BotStore } from '../../../store/bot-store'
import { haQuanto } from '../torre/formato'
import './aba-time.css'

const RELER_MS = 10_000

const ROTULO = {
  queued: 'bots.painel.time.rotulo.queued', running: 'bots.painel.time.rotulo.running', completed: 'bots.painel.time.rotulo.completed',
  restart_uncertain: 'torre.pedidos.rotulo.restart_uncertain', restart_before_start: 'torre.pedidos.rotulo.restart_before_start',
  host_stopped: 'torre.pedidos.rotulo.host_stopped', start_failed: 'torre.pedidos.rotulo.start_failed', timeout: 'torre.pedidos.rotulo.timeout',
  recipient_stopped: 'torre.pedidos.rotulo.recipient_stopped', cancelled: 'torre.pedidos.rotulo.cancelled', not_started: 'torre.pedidos.rotulo.not_started',
  failed: 'torre.pedidos.rotulo.failed', stopped: 'torre.pedidos.rotulo.stopped', incomplete: 'torre.pedidos.rotulo.incomplete',
} as const satisfies Record<TeamRequestLabel, ChaveTexto>

type Leitura = { dados: TeamRequests | null; erro: boolean }

function useTeamRequests(store: BotStore, botId: string, active: boolean): Leitura {
  const [leitura, setLeitura] = useState<Leitura>({ dados: null, erro: false })
  const geracao = useRef(0)
  useEffect(() => {
    setLeitura({ dados: null, erro: false })
    if (!active) return
    const ler = async () => {
      const minha = ++geracao.current
      const client = store.connectedClient
      try {
        if (!client) throw new Error('offline')
        const dados = parseTeamRequests(await client.call('donnaTeamRequests', { botId }), botId)
        if (minha === geracao.current) setLeitura(dados ? { dados, erro: false } : { dados: null, erro: true })
      } catch {
        if (minha === geracao.current) setLeitura((antes) => ({ dados: antes.dados, erro: true }))
      }
    }
    let ativo = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const ciclo = async () => {
      await ler()
      if (ativo) timer = setTimeout(() => { void ciclo() }, RELER_MS)
    }
    void ciclo()
    return () => { ativo = false; clearTimeout(timer); geracao.current++ }
  }, [store, botId, active])
  return leitura
}

export function AbaTime({ store, bot, active }: { store: BotStore; bot: Bot; active: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  const { dados, erro } = useTeamRequests(store, bot.id, active)
  if (!dados) return <p className="tm-empty" role={erro ? 'alert' : 'status'}>{t(erro ? 'bots.painel.time.erro' : 'bots.painel.time.carregando')}</p>
  const agora = Date.now()
  return <>
    {erro ? <p className="tm-empty" role="alert">{t('bots.painel.time.erro')}</p> : null}
    {dados.items.length === 0 ? <p className="tm-empty">{t('bots.painel.time.nenhum')}</p> : (
      <ul className="tm-list" aria-label={t('bots.painel.time.lista', { bot: bot.name })}>
        {dados.items.map((item) => {
          const colega = store.bots.get(item.targetBotId) ?? null
          const como = teamRequestLabel(item)!
          const nome = colega?.name ?? t('bots.painel.time.colegaFora')
          return (
            <li key={item.key} className="tm-row" data-pedido={item.key}>
              <span className="tm-face" aria-hidden>{colega ? <BotAvatar bot={colega} size={24} /> : null}</span>
              <span className="tm-main">
                <span className="tm-l1"><span className="tm-who" dir="auto">{nome}</span><span className={`tm-pill tone-${como.tone}`}><i aria-hidden />{t(ROTULO[como.rotulo])}</span></span>
                <span className={`tm-text${item.text === null ? ' none' : ''}`} dir="auto">{item.text ?? t('bots.painel.time.semTexto')}</span>
                <span className="tm-when">{t(`torre.pedidos.modo.${item.mode}`)} · {haQuanto(ago(item.updatedAt, agora), idioma)}</span>
              </span>
              <button className="rp-mini" disabled={!colega} title={colega ? t('bots.painel.time.abrirDica', { bot: isolado(colega.name) }) : t('bots.painel.time.colegaFora')}
                onClick={() => { if (colega) store.setSelection(colega.id) }}>{t('bots.painel.time.abrir')}</button>
            </li>
          )
        })}
      </ul>
    )}
    {dados.omitted ? <p className="tm-more">{t.plural('torre.pedidos.maisAntigos', dados.omitted)}</p> : null}
  </>
}
