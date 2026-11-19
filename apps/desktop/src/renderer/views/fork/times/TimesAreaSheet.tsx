import { useId, useMemo, useState } from 'react'
import {
  addArea, localRefusal, memberOf, moveBot, reportOfferRefusal, setReportsTo, singleLeader, subareasOf, TIMES_LIMITS, updateArea, upstreamManager,
  type TimesBot, type TimesGraph,
} from '@shared/fork-times'
import { Button } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { areaPath, nomeDoBot, Note, TF, TimesFace, useTimesTextos } from './parts'
import { cru, ErroFrase, frase, type Frase } from '../../../lib/fork-i18n/frase'
import { CampoRegras } from '../delegacao/CampoRegras'
import type { RegrasLink } from '../delegacao/regras'
import { useT } from '../../../lib/fork-i18n/estado'

// Novo time, nova subárea e Editar área: nome, gerente entre os bots que já existem e a área de cima. Na criação,
// os bots de Sem time podem entrar na área. Não cria bot nem grupo. O ID da área nova nasce aqui (UUID) e o Hub
// confere; a mudança vai como prévia, sem gravar. D454 (fatia 9c): no idioma da tela; nome e opções na direção deles.

export function TimesAreaSheet({ graph, bots, primaryBotId, nodeId, parentId, newId, onSubmit, onCancel, regras }: {
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  /** A área editada; null cria uma nova (em `parentId`, se houver). */
  nodeId: string | null
  parentId: string | null
  newId: () => string
  onSubmit: (label: Frase, change: (g: TimesGraph) => TimesGraph) => void
  onCancel: () => void
  /** D371: as regras desta equipe, gravadas à parte pela RPC do dono; só no Pessoal. `saved`: a área já está no Hub. */
  regras?: { link: RegrasLink; saved: boolean } | null
}) {
  const node = nodeId ? graph.nodes.find((n) => n.id === nodeId) ?? null : null
  const ids = useId()
  const t = useT()
  const { T, tx, nameOf } = useTimesTextos()
  const motivo = (f: Frase | null) => (f ? tx(f) : null)
  const [name, setName] = useState(node?.name ?? '')
  const [manager, setManager] = useState<string>(node?.managerBotId ?? '')
  const [parent, setParent] = useState<string>(node ? node.parentId ?? '' : parentId ?? '')
  const [joining, setJoining] = useState<Set<string>>(() => new Set())
  // As duas caixas de Reporta a vêm marcadas; desmarcar vale até trocar o gerente ou a área de cima.
  const [joinDeclined, setJoinDeclined] = useState(false)
  const [upDeclined, setUpDeclined] = useState(false)
  const alsoReport = !!manager && !joinDeclined
  const loose = useMemo(() => graph.members.filter((m) => !m.teamId && !m.parentBotId && m.botId !== primaryBotId), [graph, primaryBotId])
  const inside = useMemo(() => (nodeId ? subareasOf(graph, nodeId) : new Set<string>()), [graph, nodeId])
  const parents = graph.nodes.filter((n) => n.id !== nodeId && !inside.has(n.id))
  const trimmed = name.trim()
  // R1 do #637 (D507): os motivos ficam como Frase (dado) e só viram texto ao desenhar; trocar o idioma troca o texto.
  const nameProblem: Frase | null = !trimmed ? frase('times.quadro.area.deNome') : [...trimmed].length > TIMES_LIMITS.name ? frase('times.quadro.area.ate80') : null

  const structure = (id: string) => (g: TimesGraph): TimesGraph => {
    const managerId = manager || null
    let next = node
      ? updateArea(g, node.id, { name: trimmed, parentId: parent || null, managerBotId: managerId })
      : addArea(g, { id, name: trimmed, parentId: parent || null, managerBotId: managerId })
    if (!node && managerId) {
      // O gerente que estava em Sem time entra na área nova; quem já tem lugar (ou a principal) fica onde está.
      const m = memberOf(next, managerId)
      if (m && !m.teamId && !m.parentBotId && managerId !== primaryBotId) next = moveBot(next, managerId, { teamId: id, parentBotId: null })
    }
    for (const botId of joining) {
      if (botId === managerId) continue
      next = moveBot(next, botId, { teamId: id, parentBotId: null }, alsoReport && managerId ? managerId : null)
    }
    return next
  }
  const draftId = node?.id ?? '00000000-0000-4000-8000-000000000000'
  // D340: sem gerente escolhido, o único bot da área vira o chefe (como o Hub grava); conta para a oferta de cima.
  const settled = (id: string) => (g: TimesGraph): TimesGraph => singleLeader(structure(id)(g), g, primaryBotId)
  const chief = useMemo(() => {
    if (manager) return manager
    try {
      return settled(draftId)(graph).nodes.find((n) => n.id === draftId)?.managerBotId ?? ''
    } catch {
      return ''
    }
  }, [graph, name, manager, parent, joining, primaryBotId])
  // O gerente desta área passa a reportar ao gerente da área de cima. Na edição, só se o gerente ou o pai mudou.
  const upOffered = !!chief && !!parent && (!node || chief !== (node.managerBotId ?? '') || parent !== (node.parentId ?? ''))
  const up = upOffered ? upstreamManager(graph, { teamId: parent, parentBotId: null }) : null
  const upFrase = useMemo((): Frase | null => {
    if (!upOffered) return null
    if (!up) return frase('times.quadro.area.cimaSemGerente')
    if (up === chief) return frase('times.quadro.area.mesmoGerente')
    try {
      const base = settled(draftId)(graph)
      // Se a área em si ainda não fecha, o motivo aparece embaixo; a caixa não repete.
      return localRefusal(base, primaryBotId) ? null : reportOfferRefusal(base, chief, up, primaryBotId, nameOf(bots, up))
    } catch {
      return null
    }
  }, [graph, name, manager, chief, parent, joining, alsoReport, upOffered, up, bots])
  const upReason = motivo(upFrase)
  const upReport = upOffered && !!up && !upReason && !upDeclined
  const change = (id: string) => (g: TimesGraph): TimesGraph => {
    const next = settled(id)(g)
    return upReport && up ? setReportsTo(next, chief, { kind: 'bot', botId: up }) : next
  }
  const previewFrase = useMemo((): Frase | null => {
    if (nameProblem) return nameProblem
    try {
      return localRefusal(change(draftId)(graph), primaryBotId)
    } catch (e) {
      return e instanceof ErroFrase ? e.frase : cru(e instanceof Error ? e.message : String(e))
    }
  }, [graph, name, manager, parent, joining, alsoReport, upReport, nameProblem])
  const preview = motivo(previewFrase)
  const label: Frase = node ? frase('times.quadro.area.editarNome', { nome: node.name }) : parent ? frase('times.quadro.area.novaSubareaEm', { nome: trimmed, area: cru(areaPath(graph, parent)) }) : frase('times.quadro.area.novaEquipeNome', { nome: trimmed })

  return (
    <SheetPage className="times-sheet">
      <ModalHeader closeTitle={tx(frase('times.quadro.fechar'))} title={tx(node ? frase('times.quadro.editarAreaTitulo') : parent ? frase('times.quadro.novaSubareaTitulo') : TF.newTeam)} subtitle={tx(frase('times.quadro.area.comBots'))} />
      <div className="times-form">
        <label htmlFor={`${ids}-name`}>{tx(frase('times.quadro.area.nome'))}</label>
        <input id={`${ids}-name`} className="times-input" value={name} dir={name ? 'auto' : undefined} maxLength={TIMES_LIMITS.name} onChange={(e) => setName(e.target.value)} placeholder={tx(frase('times.quadro.area.exemplo'))} data-autofocus />
        <label htmlFor={`${ids}-manager`}>{tx(frase('times.quadro.area.gerente'))}</label>
        <select id={`${ids}-manager`} className="times-input" value={manager} onChange={(e) => { setManager(e.target.value); setJoinDeclined(false); setUpDeclined(false) }}>
          <option value="">{tx(frase('times.quadro.area.semGerente'))}</option>
          {graph.members.map((m) => <option key={m.botId} value={m.botId} dir="auto">{m.botId === primaryBotId ? tx(frase('times.quadro.area.principal', { nome: nomeDoBot(bots, m.botId) })) : nameOf(bots, m.botId)}</option>)}
        </select>
        <label htmlFor={`${ids}-parent`}>{tx(frase('times.quadro.area.areaDeCima'))}</label>
        <select id={`${ids}-parent`} className="times-input" value={parent} onChange={(e) => { setParent(e.target.value); setUpDeclined(false) }}>
          <option value="">{tx(frase('times.quadro.area.primeiroNivel'))}</option>
          {parents.map((n) => <option key={n.id} value={n.id} dir="auto">{areaPath(graph, n.id)}</option>)}
        </select>
      </div>
      {!node && loose.length ? (
        <fieldset className="times-fieldset">
          <legend>{tx(frase('times.quadro.area.entram'))}</legend>
          {loose.map((m) => (
            <label key={m.botId} className="times-check">
              <input type="checkbox" checked={joining.has(m.botId) || m.botId === manager} disabled={m.botId === manager} onChange={(e) => setJoining((s) => { const next = new Set(s); if (e.target.checked) next.add(m.botId); else next.delete(m.botId); return next })} />
              <TimesFace bot={bots.get(m.botId) ?? null} size={20} />
              <bdi dir="auto">{nameOf(bots, m.botId)}</bdi>
            </label>
          ))}
          <label className={`times-check${manager ? '' : ' is-off'}`}>
            <input type="checkbox" checked={alsoReport} disabled={!manager} onChange={(e) => setJoinDeclined(!e.target.checked)} />
            <span>{tx(manager ? frase('times.quadro.area.tambemReportam', { nome: nameOf(bots, manager) }) : frase('times.quadro.area.escolhaGerente'))}</span>
          </label>
        </fieldset>
      ) : null}
      {upOffered ? (
        <label className={`times-check${upReason ? ' is-off' : ''}`} title={upReason ?? undefined}>
          <input type="checkbox" checked={upReport} disabled={!!upReason} onChange={(e) => setUpDeclined(!e.target.checked)} aria-description={upReason ?? undefined} />
          <span>{tx(up ? frase('times.quadro.area.passaReportar', { nome: nameOf(bots, chief), alvo: nameOf(bots, up) }) : frase('times.quadro.area.passaReportarCima', { nome: nameOf(bots, chief) }))}{upReason ? <span className="times-choice-detail"> {upReason}</span> : null}</span>
        </label>
      ) : null}
      {node && regras ? (regras.saved
        ? <div className="delegacao-equipe"><CampoRegras link={regras.link} teamId={node.id} /></div>
        : <Note>{t('delegacao.equipe.antesDeSalvar')}</Note>) : null}
      {chief ? <Note>{tx(frase('times.quadro.area.viraGerente', { nome: nameOf(bots, chief) }))}</Note> : null}
      {preview && trimmed ? <p className="times-inline-error" role="alert">{preview}</p> : null}
      <div className="times-sheet-foot">
        <Button kind="secondary" onClick={onCancel}>{tx(frase('times.quadro.area.cancelar'))}</Button>
        <Button disabled={!!preview} onClick={() => onSubmit(label, change(node?.id ?? newId()))}>{T.send}</Button>
      </div>
    </SheetPage>
  )
}
