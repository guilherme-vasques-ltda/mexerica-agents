import type { ReactNode } from 'react'
import {
  areaContents, dependentsOf, localRefusal, memberOf, removeArea, setContact, setRole,
  type TimesBot, type TimesContact, type TimesGraph, type TimesRole,
} from '@shared/fork-times'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { Button, Spinner } from '../../../components/Controls'
import { AREA } from './structure'
import { areaPath, CONTACT_FRASE, nomeDoBot, Note, OptionGroup, ROLE_FRASE, TF, TimesFace, useTimesTextos } from './parts'
import { frase, juntas, plural, type Frase } from '../../../lib/fork-i18n/frase'
import type { TimesSession } from './times-session'

// O painel de detalhes do que está escolhido no Quadro ou na Lista, e a faixa de conferir e salvar. Papel e
// “Fala com você” mudam aqui; Reporta a, Mover e rotas abrem os seletores (os mesmos do menu e do teclado).
// D454 (fatia 9c): no idioma da tela; nomes de bot e de área isolados (`<bdi dir="auto">`); o que o Hub disse
// fica como `Frase` na sessão e é traduzido aqui.

export type InspectorAction = 'reports' | 'move' | 'route' | 'editArea' | 'newSubarea' | 'unteam'

export function TimesInspector({ session, graph, bots, primaryBotId, selected, editable, drawer = null, onAction, onPropose }: {
  session: TimesSession
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  selected: string | null
  editable: boolean
  /** E4: in a short window the panel is a drawer over the board, shown only while open. */
  drawer?: { open: boolean } | null
  onAction: (action: InspectorAction, key: string, invoker: HTMLElement) => void
  onPropose: (label: Frase, change: (g: TimesGraph) => TimesGraph) => void
}) {
  const { T, tx } = useTimesTextos()
  return (
    <aside id="times-side" className={`times-side${drawer ? ' is-drawer' : ''}`} aria-label={tx(frase('times.quadro.detalhes'))} hidden={drawer ? !drawer.open : undefined}>
      <div className="times-side-scroll">
        {selected && memberOf(graph, selected) ? <BotDetail graph={graph} bots={bots} primaryBotId={primaryBotId} botId={selected} editable={editable} onAction={onAction} onPropose={onPropose} />
          : selected?.startsWith(AREA) && graph.nodes.some((n) => n.id === selected.slice(AREA.length)) ? <AreaDetail graph={graph} bots={bots} nodeId={selected.slice(AREA.length)} editable={editable} onAction={onAction} onPropose={onPropose} />
          : selected === 'owner' ? <Plain title={T.owner}><Note>{tx(frase('times.quadro.insp.vocePerfil'))}</Note><Note>{tx(frase('times.quadro.insp.voceFala'))}</Note></Plain>
          : selected === 'loose' ? <Plain title={T.loose}><Note>{tx(frase('times.quadro.insp.semEquipe'))}</Note></Plain>
          : <Legend />}
      </div>
      <EditBar session={session} />
    </aside>
  )
}

function Plain({ title, children }: { title: string; children: ReactNode }) {
  return <section className="times-detail"><h2 className="times-detail-title">{title}</h2>{children}</section>
}

function Legend() {
  const { T, tx } = useTimesTextos()
  return (
    <section className="times-detail">
      <h2 className="times-detail-title">{tx(frase('times.quadro.insp.tresLigacoes'))}</h2>
      <dl className="times-legend">
        <dt><span className="times-key is-structure" aria-hidden />{T.below}</dt>
        <dd>{tx(frase('times.quadro.insp.estruturaSignifica'))}</dd>
        <dt><span className="times-key is-report" aria-hidden />{T.reportsTo}</dt>
        <dd>{tx(juntas(TF.reportsMeaning, frase('times.quadro.insp.portaSignifica')))}</dd>
        <dt><span className="times-key is-route" aria-hidden />{tx(frase('times.quadro.rotaProposta'))}</dt>
        <dd>{T.routesMeaning}</dd>
      </dl>
      <Note>{tx(juntas(TF.dragOnlyLayout, TF.noPower))}</Note>
      <Note>{tx(frase('times.quadro.insp.escolha'))}</Note>
    </section>
  )
}

function BotDetail({ graph, bots, primaryBotId, botId, editable, onAction, onPropose }: {
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  botId: string
  editable: boolean
  onAction: (action: InspectorAction, key: string, invoker: HTMLElement) => void
  onPropose: (label: Frase, change: (g: TimesGraph) => TimesGraph) => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const { T, tx, ROLE_LABEL, CONTACT_LABEL, nameOf, reportsLabel, sep } = useTimesTextos()
  const m = memberOf(graph, botId)!
  const bot = bots.get(botId) ?? null
  const name = nameOf(bots, botId)
  const primary = botId === primaryBotId
  const area = areaPath(graph, m.teamId)
  const below = graph.members.filter((x) => x.parentBotId === botId).map((x) => nameOf(bots, x.botId))
  const routes = graph.routes.filter((r) => r.fromBotId === botId).map((r) => nameOf(bots, r.toBotId))
  const deps = dependentsOf(graph, botId)
  // D454 (fatia 10b): as recusas de fork-times são frases; a tela traduz ao desenhar.
  const motivo = (f: Frase | null) => (f ? tx(f) : null)
  const roleReason = (role: TimesRole) => role === m.role ? null
    : role === 'executor' && (deps.reporters.length || deps.areas.length) ? tx(frase('times.quadro.insp.realoque'))
    : motivo(localRefusal(setRole(graph, botId, role), primaryBotId))
  const contactReason = (c: TimesContact) => c === m.ownerContact ? null
    : m.reportsTo.kind === 'owner' && c !== 'reports' ? tx(frase('times.quadro.insp.recebeRelatorios'))
    : motivo(localRefusal(setContact(graph, botId, c), primaryBotId))
  const inPlace = !!m.teamId || !!m.parentBotId
  return (
    <section className="times-detail">
      <div className="times-detail-head">
        <TimesFace bot={bot} size={40} />
        <div>
          <h2 className="times-detail-title" dir="auto">{name}</h2>
          <p className="times-detail-sub"><bdi dir="auto">{[bot?.title, area || (primary ? tx(frase('times.quadro.insp.principal')) : T.loose)].filter(Boolean).join(' · ')}</bdi></p>
        </div>
      </div>
      <dl className="times-facts">
        <dt>{T.below}</dt><dd dir="auto">{m.parentBotId ? nameOf(bots, m.parentBotId) : area ? tx(frase('times.quadro.insp.topoDe', { area })) : primary ? T.owner : T.loose}</dd>
        <dt>{T.reportsTo}</dt><dd dir="auto">{reportsLabel(bots, m)}</dd>
        <dt>{tx(frase('times.quadro.insp.papel'))}</dt><dd>{primary ? tx(frase('times.quadro.insp.principalGerente')) : ROLE_LABEL[m.role]}</dd>
        <dt>{tx(frase('times.quadro.insp.falaComVoce'))}</dt><dd>{CONTACT_LABEL[m.ownerContact]}</dd>
        {below.length ? <><dt>{tx(frase('times.quadro.insp.abaixoDele'))}</dt><dd>{below.map((b, i) => <span key={i}>{i ? sep : ''}<bdi dir="auto">{b}</bdi></span>)}</dd></> : null}
        {routes.length ? <><dt>{tx(frase('times.quadro.insp.rotas'))}</dt><dd>{routes.map((r, i) => <span key={i}>{i ? sep : ''}{tx(frase('times.quadro.insp.rotaProposta', { nome: r }))}</span>)}</dd></> : null}
      </dl>
      {primary ? (
        <Note><span {...superficieIdioma(idioma)} className="idioma-superficie">{t('times.principal.fixa')}</span></Note>
      ) : (
        <>
          <div className="times-actions">
            <Button kind="secondary" disabled={!editable} onClick={() => onAction('reports', botId, document.activeElement as HTMLElement)} title={tx(frase('times.quadro.insp.teclaR'))}>{tx(frase('times.quadro.reportaAMenu'))}</Button>
            <Button kind="secondary" disabled={!editable} onClick={() => onAction('move', botId, document.activeElement as HTMLElement)} title={tx(frase('times.quadro.insp.teclaM'))}>{tx(frase('times.quadro.moverMenu'))}</Button>
            {inPlace ? <Button kind="secondary" disabled={!editable} onClick={() => onAction('unteam', botId, document.activeElement as HTMLElement)}>{tx(frase('times.quadro.tirarDaEquipe'))}</Button> : null}
          </div>
          <h3 className="times-detail-label">{tx(frase('times.quadro.insp.papel'))}</h3>
          <OptionGroup label={tx(frase('times.quadro.insp.papel'))} value={m.role} disabled={!editable}
            options={(['manager', 'executor'] as const).map((id) => ({ id, label: ROLE_LABEL[id], reason: roleReason(id) }))}
            onChange={(role) => onPropose(frase('times.quadro.insp.nomeComo', { nome: nomeDoBot(bots, botId), papel: ROLE_FRASE[role] }), (g) => setRole(g, botId, role))} />
          <Note>{tx(frase('times.quadro.insp.gerenteExecuta'))}</Note>
          <h3 className="times-detail-label">{tx(frase('times.quadro.insp.falaComVoce'))}</h3>
          <OptionGroup label={tx(frase('times.quadro.insp.falaComVoce'))} value={m.ownerContact} disabled={!editable}
            options={(['reports', 'urgent_only', 'on_request'] as const).map((id) => ({ id, label: CONTACT_LABEL[id], reason: contactReason(id) }))}
            onChange={(c) => onPropose(frase('times.quadro.insp.nomeFala', { nome: nomeDoBot(bots, botId), contato: CONTACT_FRASE[c] }), (g) => setContact(g, botId, c))} />
          <Note>{T.contactMeaning}</Note>
        </>
      )}
      <div className="times-actions">
        <Button kind="secondary" disabled={!editable} onClick={() => onAction('route', botId, document.activeElement as HTMLElement)}>{tx(frase('times.quadro.rotaPropostaMenu'))}</Button>
      </div>
      <Note>{T.reportsMeaning}</Note>
      <Note>{tx(juntas(TF.routesMeaning, TF.noPower))}</Note>
    </section>
  )
}

function AreaDetail({ graph, bots, nodeId, editable, onAction, onPropose }: {
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  nodeId: string
  editable: boolean
  onAction: (action: InspectorAction, key: string, invoker: HTMLElement) => void
  onPropose: (label: Frase, change: (g: TimesGraph) => TimesGraph) => void
}) {
  const { tx, nameOf, sep } = useTimesTextos()
  const node = graph.nodes.find((n) => n.id === nodeId)!
  const inside = areaContents(graph, nodeId)
  const nBots = inside.bots.length ? plural('times.quadro.contagem.bots', inside.bots.length) : null
  const nAreas = inside.areas.length ? plural('times.quadro.contagem.subareas', inside.areas.length) : null
  const blocked = nBots && nAreas ? frase('times.quadro.insp.moverAntesAmbos', { bots: nBots, areas: nAreas })
    : nBots ? frase('times.quadro.insp.moverAntesBots', { bots: nBots })
    : nAreas ? frase('times.quadro.insp.moverAntesAreas', { areas: nAreas }) : null
  const key = AREA + nodeId
  return (
    <section className="times-detail">
      <h2 className="times-detail-title" dir="auto">{node.name}</h2>
      <p className="times-detail-sub">{tx(node.parentId ? frase('times.quadro.insp.subareaDe', { area: areaPath(graph, node.parentId) }) : frase('times.quadro.insp.primeiroNivel'))}</p>
      <dl className="times-facts">
        <dt>{tx(frase('times.quadro.insp.gerente'))}</dt><dd dir="auto">{node.managerBotId ? nameOf(bots, node.managerBotId) : tx(frase('times.quadro.insp.semGerente'))}</dd>
        <dt>{tx(frase('times.quadro.insp.bots'))}</dt><dd>{inside.bots.length ? inside.bots.map((id, i) => <span key={id}>{i ? sep : ''}<bdi dir="auto">{nameOf(bots, id)}</bdi></span>) : tx(frase('times.quadro.insp.nenhum'))}</dd>
        {inside.areas.length ? <><dt>{tx(frase('times.quadro.insp.subareas'))}</dt><dd>{inside.areas.map((id, i) => <span key={id}>{i ? sep : ''}<bdi dir="auto">{graph.nodes.find((n) => n.id === id)?.name ?? ''}</bdi></span>)}</dd></> : null}
      </dl>
      <div className="times-actions">
        <Button kind="secondary" disabled={!editable} onClick={() => onAction('editArea', key, document.activeElement as HTMLElement)}>{tx(frase('times.quadro.editarArea'))}</Button>
        <Button kind="secondary" disabled={!editable} onClick={() => onAction('newSubarea', key, document.activeElement as HTMLElement)}>{tx(frase('times.quadro.novaSubarea'))}</Button>
        <Button kind="secondary" disabled={!editable || !!blocked} title={blocked ? tx(blocked) : undefined} onClick={() => onPropose(frase('times.quadro.insp.removerAreaNome', { nome: node.name }), (g) => removeArea(g, nodeId))}>{tx(frase('times.quadro.insp.removerArea'))}</Button>
      </div>
      {blocked ? <Note>{tx(frase('times.quadro.insp.paraRemover', { motivo: blocked }))}</Note> : null}
      <Note>{tx(frase('times.quadro.insp.areaOrganiza'))}</Note>
    </section>
  )
}

/** A faixa de conferir e salvar: o que o Hub disse da última mudança e o que fazer agora. */
function EditBar({ session }: { session: TimesSession }) {
  const { check, save, draft, stale, changes, notice, link } = session
  const { tx } = useTimesTextos()
  let body: ReactNode = null
  if (check.kind === 'checking') {
    body = <div className="times-edit" role="status"><Spinner size={12} /><span>{tx(frase('times.quadro.insp.conferindo', { mudanca: check.label }))}</span></div>
  } else if (save.kind === 'saving') {
    body = <div className="times-edit" role="status"><Spinner size={12} /><span>{tx(frase('times.quadro.insp.salvando'))}</span></div>
  } else if (save.kind === 'unknown') {
    body = (
      <div className="times-edit is-alert" role="alert">
        <span className="times-edit-text">{tx(frase('times.quadro.insp.naoSabemos'))}</span>
        <Button kind="secondary" onClick={() => session.discard()}>{tx(frase('times.quadro.insp.descartarReler'))}</Button>
        <Button disabled={link !== 'live'} onClick={() => void session.retrySave()}>{tx(TF.retry)}</Button>
      </div>
    )
  } else if (save.kind === 'conflict' || (draft && stale)) {
    body = (
      <div className="times-edit is-alert" role="alert">
        <span className="times-edit-text">{tx(juntas(save.kind === 'conflict' ? save.message : frase('times.quadro.insp.mudouOutro'), frase('times.quadro.insp.vejaVersao')))}</span>
        {check.kind === 'refused' ? <span className="times-edit-text">{tx(frase('times.quadro.insp.rotuloMotivo', { mudanca: check.label, motivo: check.message }))}</span> : null}
        <Button kind="secondary" onClick={() => session.discard()}>{tx(frase('times.quadro.insp.verVersao'))}</Button>
        <Button disabled={link !== 'live'} onClick={() => void session.reconfirm()}>{tx(frase('times.quadro.conferirDeNovo'))}</Button>
      </div>
    )
  } else if (check.kind === 'refused') {
    body = (
      <div className="times-edit is-alert" role="alert">
        <span className="times-edit-text">{tx(juntas(frase('times.quadro.insp.rotuloMotivo', { mudanca: check.label, motivo: check.message }), frase('times.quadro.insp.nadaMudou')))}</span>
        <Button kind="secondary" onClick={() => session.dismissCheck()}>{tx(frase('times.quadro.insp.ok'))}</Button>
      </div>
    )
  } else if (save.kind === 'error') {
    body = (
      <div className="times-edit is-alert" role="alert">
        <span className="times-edit-text">{tx(juntas(save.message, frase('times.quadro.insp.rascunhoContinua')))}</span>
        <Button kind="secondary" onClick={() => session.discard()}>{tx(frase('times.quadro.insp.descartar'))}</Button>
      </div>
    )
  } else if (draft) {
    body = (
      <div className="times-edit is-draft" role="status">
        <span className="times-edit-text">{tx(plural('times.quadro.insp.conferidas', changes))}</span>
        <Button kind="secondary" onClick={() => session.discard()}>{tx(frase('times.quadro.insp.descartar'))}</Button>
        <Button disabled={!session.editable} onClick={() => void session.commit()}>{tx(frase('times.quadro.insp.salvar'))}</Button>
      </div>
    )
  } else if (notice) {
    body = <div className="times-edit" role="status"><span className="times-edit-text">{tx(notice)}</span></div>
  }
  return <div className="times-edit-slot" aria-live="polite">{body}</div>
}
