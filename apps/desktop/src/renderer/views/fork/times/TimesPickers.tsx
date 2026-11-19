import { useId, useMemo, useState } from 'react'
import {
  addRoute, localRefusal, memberOf, moveBot, removeRoute, reportOfferRefusal, setReportsTo, singleLeader, subbotsOf, upstreamManager,
  type MoveTarget, type TimesBot, type TimesGraph, type TimesReportsTo,
} from '@shared/fork-times'
import { Button } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { areaPath, nomeDoBot, Note, TF, TimesFace, useTimesTextos } from './parts'
import { cru, frase, juntas, plural, type Frase } from '../../../lib/fork-i18n/frase'
import { localeDe } from '../../../lib/fork-i18n/index'

// Os equivalentes de teclado e menu de cada gesto do quadro: Reporta a (a porta), Mover (a estrutura) e Rota
// proposta. Cada opção é conferida aqui contra as regras do Hub só para desativar o que ele recusaria, com o motivo;
// escolher envia a intenção para o Hub conferir (prévia), sem gravar. D454 (fatia 9c): no idioma da tela; nomes na
// direção deles; o rótulo da mudança vai à sessão como `Frase` (D507).

export interface PickerProps {
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  botId: string
  onSubmit: (label: Frase, change: (g: TimesGraph) => TimesGraph) => void
  onCancel: () => void
}

interface Choice<V> { key: string; value: V; label: string; detail?: string; face?: TimesBot | null; reason: string | null; destino?: Frase }

function ChoiceList<V>({ label, choices, selected, onSelect }: { label: string; choices: Choice<V>[]; selected: string | null; onSelect: (key: string) => void }) {
  const name = useId()
  return (
    <div className="times-choices" role="radiogroup" aria-label={label}>
      {choices.map((c) => (
        <label key={c.key} className={`times-choice${c.reason ? ' is-off' : ''}`} title={c.reason ?? undefined}>
          <input type="radio" name={name} checked={selected === c.key} disabled={!!c.reason} onChange={() => onSelect(c.key)} aria-description={c.reason ?? undefined} />
          {c.face !== undefined ? <TimesFace bot={c.face} size={22} /> : null}
          <span className="times-choice-text">
            <bdi dir="auto">{c.label}</bdi>
            {c.reason ? <span className="times-choice-detail">{c.reason}</span> : c.detail ? <span className="times-choice-detail">{c.detail}</span> : null}
          </span>
        </label>
      ))}
    </div>
  )
}

function Footer({ ready, onSubmit, onCancel }: { ready: boolean; onSubmit: () => void; onCancel: () => void }) {
  const { T, tx } = useTimesTextos()
  return (
    <div className="times-sheet-foot">
      <Button kind="secondary" onClick={onCancel}>{tx(frase('times.quadro.pick.cancelar'))}</Button>
      <Button onClick={onSubmit} disabled={!ready}>{T.send}</Button>
    </div>
  )
}

/** Reporta a…: Você ou um bot com papel Gerente; o que fecharia um círculo fica desativado com o motivo. */
export function ReportsPicker({ graph, bots, primaryBotId, botId, onSubmit, onCancel }: PickerProps) {
  const { T, tx, nameOf } = useTimesTextos()
  const motivo = (f: Frase | null) => (f ? tx(f) : null)
  const member = memberOf(graph, botId)
  const current = member?.reportsTo.kind === 'bot' ? member.reportsTo.botId : 'owner'
  const choices = useMemo(() => {
    const out: Choice<TimesReportsTo>[] = [{ key: 'owner', value: { kind: 'owner' }, label: T.owner, detail: tx(frase('times.quadro.pick.recebeRelatorios')), reason: null }]
    for (const m of graph.members) {
      if (m.botId === botId) continue
      const value: TimesReportsTo = { kind: 'bot', botId: m.botId }
      out.push({ key: m.botId, value, label: nameOf(bots, m.botId), face: bots.get(m.botId) ?? null, detail: tx(frase(m.botId === primaryBotId ? 'times.quadro.pick.principal' : 'times.quadro.pick.gerente')), reason: m.role !== 'manager' ? tx(frase('times.quadro.pick.naoGerente')) : null })
    }
    for (const c of out) if (!c.reason && c.key !== current) c.reason = motivo(localRefusal(setReportsTo(graph, botId, c.value), primaryBotId))
    return out.sort((a, b) => Number(!!a.reason) - Number(!!b.reason))
  }, [graph, bots, botId, primaryBotId, current, T, tx, nameOf])
  const [selected, setSelected] = useState<string>(current)
  const choice = choices.find((c) => c.key === selected)
  return (
    <SheetPage className="times-sheet">
      <ModalHeader closeTitle={tx(frase('times.quadro.fechar'))} title={tx(frase('times.quadro.pick.reportaTitulo', { nome: nameOf(bots, botId) }))} subtitle={tx(frase('times.quadro.pick.reportaSub'))} />
      <ChoiceList label={T.reportsTo} choices={choices} selected={selected} onSelect={setSelected} />
      <Note>{tx(juntas(TF.reportsMeaning, frase('times.quadro.pick.moverOferece')))}</Note>
      <Footer ready={!!choice && !choice.reason && selected !== current} onCancel={onCancel} onSubmit={() => choice && onSubmit(frase('times.quadro.nomeReportaA', { nome: nomeDoBot(bots, botId), alvo: choice.key === 'owner' ? TF.owner : nomeDoBot(bots, choice.key) }), (g) => setReportsTo(g, botId, choice.value))} />
    </SheetPage>
  )
}

/** Mover…: muda onde o bot fica; a subárvore vai junto. Reporta a ao nível de cima vem marcado quando é válido. */
export function MovePicker({ graph, bots, primaryBotId, botId, onSubmit, onCancel }: PickerProps) {
  const { T, tx, nameOf, idioma } = useTimesTextos()
  const motivo = (f: Frase | null) => (f ? tx(f) : null)
  const member = memberOf(graph, botId)
  const below = useMemo(() => subbotsOf(graph, botId), [graph, botId])
  const choices = useMemo(() => {
    const out: Choice<MoveTarget>[] = [{ key: 'loose', value: { teamId: null, parentBotId: null }, label: T.loose, detail: tx(frase('times.quadro.pick.foraAreas')), reason: null, destino: TF.loose }]
    for (const n of [...graph.nodes].sort((a, b) => areaPath(graph, a.id).localeCompare(areaPath(graph, b.id), localeDe(idioma)))) {
      out.push({ key: `area:${n.id}`, value: { teamId: n.id, parentBotId: null }, label: tx(frase('times.quadro.pick.topoDe', { area: areaPath(graph, n.id) })), reason: null, destino: cru(areaPath(graph, n.id)) })
    }
    for (const m of graph.members) {
      if (m.botId === botId) continue
      const area = areaPath(graph, m.teamId)
      out.push({ key: m.botId, value: { teamId: null, parentBotId: m.botId }, label: tx(frase('times.quadro.pick.abaixoDe', { nome: nameOf(bots, m.botId) })), detail: area || T.loose, face: bots.get(m.botId) ?? null, reason: below.has(m.botId) ? tx(frase('times.quadro.pick.estaAbaixo')) : null })
    }
    const here = member ? (member.parentBotId ?? (member.teamId ? `area:${member.teamId}` : 'loose')) : null
    for (const c of out) {
      if (c.key === here) c.reason = tx(frase('times.quadro.pick.jaEsta'))
      if (!c.reason) c.reason = motivo(localRefusal(moveBot(graph, botId, c.value), primaryBotId))
    }
    return out
  }, [graph, bots, botId, primaryBotId, below, member, T, tx, nameOf, idioma])
  const [selected, setSelected] = useState<string | null>(null)
  // Desmarcar vale só para o destino escolhido; trocar de destino volta ao padrão (marcado, se a oferta for válida).
  const [declined, setDeclined] = useState(false)
  const choice = choices.find((c) => c.key === selected) ?? null
  // O destinatário oferecido: o bot de cima, ou o gerente da área de destino; só se for gerente e sem círculo.
  const offer = choice ? upstreamManager(graph, choice.value) : null
  // D340: o membro único sem chefe já conta como o chefe da área (o Hub grava assim).
  const offerReason = choice && offer ? motivo(reportOfferRefusal(moveBot(singleLeader(graph), botId, choice.value), botId, offer, primaryBotId, nameOf(bots, offer))) : null
  const alsoReport = !!offer && !offerReason && !declined
  const reportsNow = member?.reportsTo.kind === 'bot' ? nameOf(bots, member.reportsTo.botId) : T.owner
  return (
    <SheetPage className="times-sheet">
      <ModalHeader closeTitle={tx(frase('times.quadro.fechar'))} title={tx(frase('times.quadro.pick.moverTitulo', { nome: nameOf(bots, botId) }))} subtitle={tx(below.size ? plural('times.quadro.pick.subbotsJunto', below.size) : frase('times.quadro.pick.mudaOnde'))} />
      <ChoiceList label={tx(frase('times.quadro.pick.paraOnde'))} choices={choices} selected={selected} onSelect={(key) => { setSelected(key); setDeclined(false) }} />
      <label className={`times-check${!offer || offerReason ? ' is-off' : ''}`} title={offerReason ?? undefined}>
        <input type="checkbox" checked={alsoReport} disabled={!offer || !!offerReason} onChange={(e) => setDeclined(!e.target.checked)} aria-description={offerReason ?? undefined} />
        <span>{tx(offer ? frase('times.quadro.pick.tambemReportar', { nome: nameOf(bots, offer) }) : frase('times.quadro.pick.tambemReportarVazio'))}{offerReason ? <span className="times-choice-detail"> {offerReason}</span> : null}</span>
      </label>
      <Note>{tx(frase('times.quadro.pick.hojeReporta', { nome: reportsNow }))}</Note>
      <Footer ready={!!choice && !choice.reason} onCancel={onCancel} onSubmit={() => {
        if (!choice) return
        const report = alsoReport ? offer : null
        onSubmit(choice.destino ? frase('times.quadro.pick.moverPara', { nome: nomeDoBot(bots, botId), destino: choice.destino }) : frase('times.quadro.pick.moverAbaixo', { nome: nomeDoBot(bots, botId), destino: nomeDoBot(bots, choice.key) }), (g) => moveBot(report ? singleLeader(g) : g, botId, choice.value, report))
      }} />
    </SheetPage>
  )
}

/** Rota proposta…: anota uma rota para o futuro, ou tira uma. Não muda quem pode chamar quem hoje. */
export function RoutePicker({ graph, bots, primaryBotId, botId, onSubmit, onCancel }: PickerProps) {
  const { tx, nameOf } = useTimesTextos()
  const existing = new Set(graph.routes.filter((r) => r.fromBotId === botId).map((r) => r.toBotId))
  const choices: Choice<string>[] = graph.members.filter((m) => m.botId !== botId).map((m) => ({
    key: m.botId, value: m.botId, label: nameOf(bots, m.botId), face: bots.get(m.botId) ?? null,
    detail: existing.has(m.botId) ? tx(frase('times.quadro.pick.anotada')) : undefined, reason: null,
  }))
  const [selected, setSelected] = useState<string | null>(null)
  const removing = selected !== null && existing.has(selected)
  return (
    <SheetPage className="times-sheet">
      <ModalHeader closeTitle={tx(frase('times.quadro.fechar'))} title={tx(frase('times.quadro.pick.rotaTitulo', { nome: nameOf(bots, botId) }))} subtitle={tx(frase('times.quadro.pick.rotaSub'))} />
      <ChoiceList label={tx(frase('times.quadro.pick.paraQuem'))} choices={choices} selected={selected} onSelect={setSelected} />
      <Note>{tx(juntas(TF.routesMeaning, TF.noPower))}</Note>
      <div className="times-sheet-foot">
        <Button kind="secondary" onClick={onCancel}>{tx(frase('times.quadro.pick.cancelar'))}</Button>
        <Button disabled={!selected} onClick={() => {
          if (!selected) return
          const to = nomeDoBot(bots, selected)
          if (removing) onSubmit(frase('times.quadro.pick.tirarRota', { nome: to }), (g) => removeRoute(g, botId, selected))
          else onSubmit(frase('times.quadro.pick.rotaPara', { nome: to }), (g) => addRoute(g, botId, selected))
        }}>{tx(frase(removing ? 'times.quadro.pick.tirarProposta' : 'times.quadro.pick.anotar'))}</Button>
      </div>
      {primaryBotId === botId ? <Note>{tx(frase('times.quadro.pick.principalRotas'))}</Note> : null}
    </SheetPage>
  )
}
