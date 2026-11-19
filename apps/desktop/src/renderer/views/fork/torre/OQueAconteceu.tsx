// MB-3: a coluna "O que aconteceu" do Mission Board, à direita dos blocos
// e fora dos contadores. Só desenha `whatHappened` do snapshot: o último dia fechado, por Time (linha do Time e uma linha
// por bot), "Bots sem time" (D329) e quem ficou fora do resumo. Sem "atualizar agora". O único controle é o horário e o
// liga/desliga, gravados pela RPC do dono `donnaResumoDiarioSave`.
import { memo, useState } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { TowerBot, TowerSnapshot } from '@shared/fork-torre'
import { deviceTimeZone, longDay, nextDay, relativeDay, TIMES, type RelativeDay, type SummaryBot, type SummaryLine } from '@shared/fork-torre-resumo'
import { isolado, ltr } from '@shared/bidi'
import type { AppLanguage } from '@shared/fork-idioma'
import { localeDe } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { Icon } from '../../../components/Icon'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import './o-que-aconteceu.css'

export interface SummarySave { enabled: boolean; time: string; timeZone: string }

const Face = memo(function Face({ bot }: { bot: TowerBot }) {
  const config = normalizarAvatarDoBot(avatarDoBot(bot), bot.name)
  return <DonnabotAvatar config={config} size={22} animated={false} label={bot.name} />
})

/** Hoje/ontem/amanhã pelo `Intl.RelativeTimeFormat` do idioma; outra data, por extenso no idioma. */
function dayText(d: RelativeDay, idioma: AppLanguage) {
  return 'offset' in d ? new Intl.RelativeTimeFormat(localeDe(idioma), { numeric: 'auto' }).format(d.offset, 'day') : longDay(d.day, localeDe(idioma))
}

function Joined({ line }: { line: SummaryLine }) {
  const t = useT()
  return line.joined ? <span className="oqa-joined">{t('torre.oqa.junto')}</span> : null
}

function Row({ item, byId }: { item: SummaryBot; byId: Map<string, TowerBot> }) {
  const bot = byId.get(item.botId)
  if (!bot) return null
  return (
    <li className="oqa-bot">
      <Face bot={bot} />
      <p><b><bdi>{bot.name}</bdi></b> <span dir="auto">{item.line.text}</span><Joined line={item.line} /></p>
    </li>
  )
}

export function OQueAconteceu({ snapshot, personal, onSave }: { snapshot: TowerSnapshot; personal: boolean; onSave?: ((body: SummarySave) => Promise<unknown>) | null }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState<SummarySave | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(false)
  const t = useT()
  const idioma = useIdioma()
  const observed = snapshot.whatHappened
  if (!personal) {
    return (
      <section className="oqa" aria-label={t('torre.oqa.titulo')}>
        <h2 className="oqa-title">{t('torre.oqa.titulo')}</h2>
        <p className="oqa-empty">{t('torre.oqa.soPessoal')}</p>
      </section>
    )
  }
  if (observed.availability !== 'available') {
    return (
      <section className="oqa" aria-label={t('torre.oqa.titulo')}>
        <h2 className="oqa-title">{t('torre.oqa.titulo')}</h2>
        <p className="oqa-empty"><span className="torre-pill is-plain">{t('torre.estado.unknown')}</span> {t('torre.oqa.naoInformou')}</p>
      </section>
    )
  }
  const v = observed.value
  const byId = new Map(snapshot.bots.map((b) => [b.botId, b]))
  const when = dayText(relativeDay(v.day, snapshot.collectedAt, v.timeZone), idioma)
  const next = dayText(nextDay(v.nextAt, snapshot.collectedAt, v.timeZone), idioma)
  const teams = v.teams.filter((t) => t.line || t.bots.some((b) => byId.has(b.botId)))
  const loose = v.loose.filter((b) => byId.has(b.botId))
  const out = v.out.filter((o) => byId.has(o.botId))
  const nothing = teams.length === 0 && loose.length === 0
  const current = draft ?? { enabled: v.enabled, time: v.time, timeZone: deviceTimeZone() }
  const save = async () => {
    if (!onSave) return
    setBusy(true)
    setError(false)
    try {
      await onSave(current)
      setEditing(false)
      setDraft(null)
    } catch {
      setError(true)
    } finally {
      setBusy(false)
    }
  }
  return (
    <section className="oqa" aria-label={t('torre.oqa.titulo')}>
      <h2 className="oqa-title">{t('torre.oqa.cabecalho', { quando: when, hora: ltr(v.time) })}</h2>
      <p className="oqa-sub">{t('torre.oqa.sub', { dia: longDay(v.day, localeDe(idioma)) })}</p>
      {!v.enabled ? <p className="oqa-empty">{t('torre.oqa.desligado')}</p> : null}
      {v.enabled && nothing ? <p className="oqa-empty">{t('torre.oqa.nada')}</p> : null}
      {teams.map((team) => (
        <div key={team.teamId} className="oqa-block">
          <h3><Icon name="person.2" size={13} /> {t('torre.oqa.equipe', { nome: isolado(team.name) })}</h3>
          {team.line ? <p className="oqa-team-line"><span dir="auto">{team.line.text}</span><Joined line={team.line} /></p> : null}
          <ul>{team.bots.map((b) => <Row key={b.botId} item={b} byId={byId} />)}</ul>
        </div>
      ))}
      {loose.length ? (
        <div className="oqa-block">
          <h3><Icon name="person.fill" size={13} /> {t('torre.oqa.semEquipe')}</h3>
          <ul>{loose.map((b) => <Row key={b.botId} item={b} byId={byId} />)}</ul>
        </div>
      ) : null}
      {out.map((o) => {
        const bot = byId.get(o.botId)!
        return (
          <div key={o.botId} className="oqa-out">
            <Face bot={bot} />
            <p><b>{t('torre.oqa.fora')}</b> {t('torre.oqa.foraDetalhe', { nome: isolado(bot.name), motivo: t(`torre.oqa.motivo.${o.reason}`) })}</p>
          </div>
        )
      })}
      <div className="oqa-foot">
        <p><Icon name="info.circle" size={13} /> {t('torre.oqa.rodape')} · {v.enabled ? t('torre.oqa.proximo', { quando: next, hora: ltr(v.time) }) : t('torre.oqa.desligadoCurto')}</p>
        {onSave && !editing ? <button type="button" className="oqa-link" onClick={() => setEditing(true)}>{t('torre.oqa.horario')}</button> : null}
      </div>
      {onSave && editing ? (
        <div className="oqa-edit" role="group" aria-label={t('torre.oqa.horarioGrupo')}>
          <label className="oqa-check"><input type="checkbox" checked={current.enabled} onChange={(e) => setDraft({ ...current, enabled: e.target.checked })} /> {t('torre.oqa.fazer')}</label>
          <label>{t('torre.oqa.fechaAs')} <select dir="ltr" value={current.time} disabled={!current.enabled} onChange={(e) => setDraft({ ...current, time: e.target.value })}>{TIMES.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
          <small>{t('torre.oqa.fuso', { fuso: ltr(current.timeZone) })}</small>
          {error ? <small className="oqa-error" role="alert">{t('torre.oqa.erroSalvar')}</small> : null}
          <div className="oqa-actions">
            <button type="button" className="torre-button" onClick={() => { setEditing(false); setDraft(null); setError(false) }}>{t('torre.oqa.cancelar')}</button>
            <button type="button" className="torre-button is-primary" disabled={busy} onClick={() => void save()}>{t('torre.oqa.salvar')}</button>
          </div>
        </div>
      ) : null}
    </section>
  )
}
