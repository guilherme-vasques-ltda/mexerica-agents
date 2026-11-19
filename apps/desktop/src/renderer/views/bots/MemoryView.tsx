import { Fragment, useEffect, useMemo, useState } from 'react'
import { Button, CardSection, DropdownMenu, IconButton, Switch, TabChoice } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader, Sheet } from '../../components/Overlay'
import { HStack } from '../../components/Stack'
import { useModel } from '../../lib/observable'
import { useStore } from '../../store/context'
import { useScreenViewed } from '../../store/analytics'
import { ltr } from '@shared/bidi'
import { MemoryStore, authorLabel, type MemoryEntry, type MemoryLayer } from '../../store/memory-store'
import { FormSheet } from '../marketplace/pieces'
import { MemoryOrigin } from '../fork/memoria-auto/MemoryOrigin'
import { automaticText, dataMemoria } from '../fork/memoria-auto/presentation'
import { Autor, ErroDaMemoria, MemoryNature, MemoryPrevious, MemoryVersionMeta, TextoMarcado } from '../fork/memoria-auto/MemoryNature'
import { NonSavedHistory } from '../fork/memoria-auto/NonSavedHistory'
import { OpeningOmitted } from '../fork/memoria-auto/OpeningOmitted'
import { Sugestoes } from '../fork/memoria-auto/SecaoSugestoes'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from '../settings/rotulo'
import { settingsScreen } from '../fork/ajustes/tela'
import { HarnessMemorySheet } from './HarnessMemorySheet'
import { type FileTarget, MemoryFileSheet, fileRows } from './MemoryFiles'
import './memory.css'

type Section = 'bot' | 'suggestions' | 'nonSaved' | 'openingOmitted' | 'dreams' | 'harness'

/** A bot's memory: what this one knows, its dreams, and its harness's own notes when the user kept them on.
 * MX-8: "Sobre você" (what all bots share) lives in Ajustes → Memória; here only whether this bot uses it. */
export function MemoryCard({ botId }: { botId: string }) {
  const host = useStore()
  const memory = useModel(useMemo(() => new MemoryStore(botId, () => host.ready()), [botId, host]))
  useScreenViewed('memory')
  const t = useT()
  const idioma = useIdioma()
  const language = superficieIdioma(idioma)
  const bot = host.bots.get(botId)
  const [section, setSection] = useState<Section>('bot')
  const [draft, setDraft] = useState<{ id?: number; text: string; shared: boolean } | null>(null)
  // Qual camada o "Esquecer tudo aqui" apaga: a deste bot ou a compartilhada.
  const [confirmClear, setConfirmClear] = useState<'bot' | 'yours' | null>(null)
  const [file, setFile] = useState<FileTarget | null>(null)
  const [aboutHarness, setAboutHarness] = useState(false)
  // Memory changes anywhere move the bot's `memoryRev`: fetch again.
  useEffect(() => { void memory.load() }, [memory, bot?.memoryRev, bot?.sharedMemory, bot?.harnessMemory])

  const view = memory.view
  const automatic = automaticText(t, view?.automatic)
  // Claude Code's section goes when its memory is turned off (on another device too).
  useEffect(() => { if (section === 'harness' && view && !view.projectMemory) setSection('bot') }, [section, view])
  // D389: aparece no modo sugestão ou enquanto houver pendente de antes da troca.
  const suggestions = memory.suggestions !== null && (view?.learning === 'suggest' || memory.suggestions.length > 0)
  useEffect(() => { if (section === 'suggestions' && view && !suggestions) setSection('bot') }, [section, view, suggestions])
  const harness = host.backendName(bot?.backend ?? 'claude')
  const sections: { id: Section; label: string }[] = [
    { id: 'bot', label: t('memoria.painel.aba.bot') },
    ...(suggestions ? [{ id: 'suggestions' as const, label: t('memoria.sugestoes.aba') }] : []),
    { id: 'nonSaved', label: t('memoria.painel.aba.naoSalvei') }, { id: 'openingOmitted', label: t('memoria.painel.aba.foraAbertura') },
    { id: 'dreams', label: t('memoria.painel.aba.consolidacoes') },
    ...(view?.projectMemory ? [{ id: 'harness' as const, label: ltr(harness) }] : []),
  ]
  const layer = section === 'bot' ? view?.bot : undefined
  const save = () => {
    if (!draft?.text.trim()) return
    void memory.mutate('saveMemory', { id: draft.id, shared: draft.shared, text: draft.text.trim() }).then((ok) => { if (ok) setDraft(null) })
  }

  /** Uma camada: nota com uso e ações, vazio, lembranças e notas longas. `shared`: a compartilhada sobre você. */
  const camada = (shared: boolean, l: MemoryLayer) => {
    const k = shared ? 'yours-' : ''
    return [
      <div key={`${k}note`} className="memory-layer-note">
        <p className="memory-muted">{t.plural(shared ? 'memoria.painel.camada.voce' : 'memoria.painel.camada.bot', l.used, { limite: l.cap })}</p>
        <div className="memory-header-actions">
          <IconButton title={t(shared ? 'memoria.painel.editor.adicionarCompartilhada' : 'memoria.painel.adicionar')} icon="plus" onClick={() => setDraft({ text: '', shared })} disabled={memory.busy} />
          <DropdownMenu className="icon-button memory-menu" title={t('memoria.painel.mais')} language={language} items={() => [
            { title: t('memoria.painel.atualizar'), icon: 'arrow.clockwise', action: () => void memory.load() },
            { title: t('memoria.painel.esquecerTudoAqui'), icon: 'trash', destructive: true, divider: true, action: () => setConfirmClear(shared ? 'yours' : 'bot') },
          ]}><Icon name="ellipsis" size={14} weight="medium" /></DropdownMenu>
        </div>
      </div>,
      ...(l.entries.length ? [] : [<div key={`${k}empty`} className="memory-empty">
        <Icon name="brain" size={20} color="var(--text-tertiary, var(--ink-3))" />
        <span>{t(shared ? 'memoria.painel.vazio.voce' : 'memoria.painel.vazio.bot')}</span>
      </div>]),
      ...l.entries.map((entry) => <EntryRow key={`${k}${entry.id}`} entry={entry} memory={memory} edit={() => setDraft({ id: entry.id, text: entry.text, shared })} />),
      ...(view?.files ? fileRows(t, memory, view.files, shared, setFile).map((row) => <Fragment key={`${k}${row.key}`}>{row}</Fragment>) : []),
    ]
  }

  return <div className="idioma-superficie" {...language} style={{ display: 'contents' }}>
    <div className="memory-panel">
      <TabChoice label={t('memoria.painel.rotulo')} selection={section} options={sections} onChange={(next) => {
        setSection(next)
        // Claude Code writes its notes outside Codync, so nothing moves `memoryRev`: read them again on the way in.
        if (next === 'harness') void memory.load()
      }} />
      <CardSection>
        {layer && <p className="memory-auto-policy">{t('memoria.painel.politica')}</p>}
        {layer && automatic && <div className="memory-muted" role="status">
          <strong>{automatic.title}</strong>
          <p>{automatic.detail}</p>
        </div>}
        {memory.error && section !== 'nonSaved' && section !== 'openingOmitted' && <div className="memory-error-row">
          <p role="alert" className="memory-error"><ErroDaMemoria m={memory.error} /></p>
          {/* D169: the same retry as the other loading errors (Rotinas, Aplicativos). */}
          {!view ? <Button kind="secondary" onClick={() => { void memory.load() }} disabled={memory.busy}>{t('memoria.painel.tentarDeNovo')}</Button> : null}
        </div>}
        {!view && !memory.error && section !== 'nonSaved' && section !== 'openingOmitted' && <p className="memory-muted">{t('memoria.painel.carregando')}</p>}
        {section === 'nonSaved' && <NonSavedHistory history={memory.nonSaved} />}
        {section === 'openingOmitted' && <OpeningOmitted history={memory.openingOmitted} view={view} />}
        {section === 'suggestions' && memory.suggestions && <Sugestoes memory={memory} items={memory.suggestions} />}
        {view && bot && section === 'bot' && <div className="memory-sobre-voce">
          <span className="memory-sobre-voce-texto">
            <span>{t('memoria.painel.usarSobreVoce')}</span>
            <small className="memory-muted">{t('memoria.painel.usarSobreVoce.detalhe')}</small>
          </span>
          <button type="button" className="settings-link" onClick={() => settingsScreen.show('memory')}>{t('memoria.painel.abrirSobreVoce')}</button>
          <Switch on={bot.sharedMemory} disabled={memory.busy} onChange={(on) => host.setSharedMemory(bot, on)} label={t('memoria.painel.usarSobreVoce')} />
        </div>}
        {view && bot && section === 'bot' && bot.harnessMemory !== null && <HStack gap={4}>
          <span>{t('memoria.painel.motor.propria', { motor: harness })}</span>
          <IconButton title={t('memoria.painel.motor.sobre', { motor: harness })} icon="info.circle" onClick={() => setAboutHarness(true)} />
          <Switch on={bot.harnessMemory} disabled={memory.busy} onChange={(on) => host.setHarnessMemory(bot, on)} label={t('memoria.painel.motor.propria', { motor: harness })} className="memory-harness-switch" />
        </HStack>}
        {layer && camada(false, layer)}
        {/* MX-8: o compartilhado sobre você continua aqui até o Hub ter a RPC global de listar/editar/excluir; aí migra para Ajustes → Memória. */}
        {layer && view?.yours && [<h4 key="yours-title" className="memory-notes-header memory-compartilhado">{t('memoria.painel.compartilhado.titulo')}</h4>, ...camada(true, view.yours)]}
        {view && section === 'bot' && view.sessionDiagnostic?.marked && <p className="memory-muted" role="status">
          {comNos(view.sessionDiagnostic.at
            ? t('memoria.painel.sessao.externaEm', { motivo: no('motivo'), data: no('data') })
            : t('memoria.painel.sessao.externa', { motivo: no('motivo') }), {
            motivo: view.sessionDiagnostic.reason ? <bdi dir="auto">{view.sessionDiagnostic.reason}</bdi> : t('memoria.painel.sessao.origemDesconhecida'),
            data: view.sessionDiagnostic.at ? <bdi>{dataMemoria(idioma, view.sessionDiagnostic.at)}</bdi> : null,
          })}
        </p>}
        {view && section === 'dreams' && view.discardedDreamChanges && <p className="memory-muted" role="status">
          {comNos(t('memoria.painel.descartadas', { data: no('data'), arquivar: ltr(view.discardedDreamChanges.archive), revisar: ltr(view.discardedDreamChanges.review), mover: ltr(view.discardedDreamChanges.move), fatos: ltr(view.discardedDreamChanges.facts ?? 0) }), {
            data: <bdi>{dataMemoria(idioma, view.discardedDreamChanges.at)}</bdi>,
          })}
        </p>}
        {view && section === 'dreams' && [
          <p key="note" className="memory-muted">{t('memoria.painel.consolidacoes.nota')}</p>,
          ...(view.dreams.length ? [] : [<div key="empty" className="memory-empty"><Icon name="moon" size={20} color="var(--text-tertiary, var(--ink-3))" /><span>{t('memoria.painel.consolidacoes.vazio')}</span></div>]),
          ...view.dreams.map((dream) => <div className="memory-item fade-in" key={dream.id}>
            <div className="memory-content">
              <strong>{dream.undone
                ? comNos(t('memoria.painel.consolidacoes.desfeita', { data: no('data') }), { data: <bdi>{dataMemoria(idioma, dream.at)}</bdi> })
                : <bdi>{dataMemoria(idioma, dream.at)}</bdi>}</strong>
              {dream.added.map((c) => <span key={`a${c.id}`} className="line-clamp memory-clamp" title={c.text}>+ <bdi dir="auto">{c.text}</bdi></span>)}
              {dream.archived.map((c) => <span key={`r${c.id}`} className="line-clamp memory-clamp memory-archived" title={c.text}>− <bdi dir="auto">{c.text}</bdi></span>)}
              {(dream.files ?? []).map((f) => <span key={`f${f.version}`} className="memory-muted">{comNos(t('memoria.painel.consolidacoes.adicionadoA', { arquivo: no('arquivo') }), { arquivo: <bdi dir="ltr">{f.name}</bdi> })}</span>)}
            </div>
            {!dream.undone && <IconButton title={t('memoria.painel.consolidacoes.desfazer')} icon="arrow.uturn.backward" disabled={memory.busy} onClick={() => void memory.mutate('undoDream', { id: dream.id })} />}
          </div>),
        ]}
        {view?.projectMemory && section === 'harness' && <div className="memory-content">
          <p className="memory-muted">{t('memoria.painel.motor.notasClaude')}</p>
          <small className="memory-muted bidi-isolado" dir="ltr">{view.projectMemory.path}</small>
          {view.projectMemory.text
            ? <pre className="memory-history" dir="auto">{view.projectMemory.text}</pre>
            : <div className="memory-empty"><Icon name="brain" size={20} color="var(--text-tertiary, var(--ink-3))" /><span>{t('memoria.painel.motor.vazioClaude')}</span></div>}
        </div>}
      </CardSection>
    </div>
    <Sheet language={language} open={draft !== null} onClose={() => setDraft(null)} width={520}>
      <div className="idioma-superficie" {...language} style={{ display: 'contents' }}>
      <ModalHeader closeTitle={t('memoria.painel.fechar')} icon="brain" title={t(draft?.id ? 'memoria.painel.editor.editar' : draft?.shared ? 'memoria.painel.editor.adicionarCompartilhada' : 'memoria.painel.adicionar')} />
      {draft && <div className="memory-editor">
        <textarea aria-label={t('memoria.painel.rotulo')} dir="auto" rows={3} autoFocus placeholder={t('memoria.painel.editor.placeholder')} value={draft.text}
          onChange={(e) => setDraft({ ...draft, text: e.target.value })}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); save() } }} />
        {memory.error && <p role="alert" className="memory-error"><ErroDaMemoria m={memory.error} /></p>}
        <Button disabled={memory.busy || !draft.text.trim()} onClick={save}>{memory.busy ? t('memoria.painel.salvando') : t('memoria.painel.salvar')}</Button>
      </div>}
      </div>
    </Sheet>
    <Sheet language={language} open={file !== null} onClose={() => setFile(null)} width={640}>
      {file && <div className="idioma-superficie" {...language} style={{ display: 'contents' }}><MemoryFileSheet memory={memory} target={file} onClose={() => setFile(null)} /></div>}
    </Sheet>
    <FormSheet language={language} open={aboutHarness} onClose={() => setAboutHarness(false)} height={460}>
      <div className="idioma-superficie" {...language} style={{ display: 'contents' }}><HarnessMemorySheet harness={harness} readable={bot?.backend === 'claude'} /></div>
    </FormSheet>
    <Dialog language={language} cancel={t('memoria.painel.cancelar')} open={confirmClear !== null}
      title={t(confirmClear === 'yours' ? 'memoria.painel.esquecer.voce' : 'memoria.painel.esquecer.bot')}
      actions={[{ title: t('memoria.painel.esquecer.tudo'), destructive: true, action: () => { void memory.mutate('clearMemory', { shared: confirmClear === 'yours' }) } }]}
      onClose={() => setConfirmClear(null)} />
  </div>
}

function EntryRow({ entry, memory, edit }: { entry: MemoryEntry; memory: MemoryStore; edit: () => void }) {
  const host = useStore()
  const t = useT()
  const language = superficieIdioma(useIdioma())
  const archived = entry.state === 'archived'
  return <div className="memory-item fade-in">
    <div className="memory-content">
      <button className="press memory-content" onClick={edit}>
        <strong className={archived ? 'memory-archived' : undefined}>{entry.pinned && <Icon name="pin.fill" size={10} color="var(--context-ink-2, var(--ink-2))" />}<span className="line-clamp memory-clamp" title={entry.text}>{entry.nature === 'inferred' ? <TextoMarcado chave="memoria.auto.hipotese" texto={entry.text} /> : entry.nature === 'conflict' ? <TextoMarcado chave="memoria.auto.anterior" texto={entry.text} /> : <bdi dir="auto">{entry.text}</bdi>}</span></strong>
        <MemoryNature nature={entry.nature} bot={entry.natureBot} name={(id) => host.bots.get(id)?.name} />
        <MemoryOrigin origin={entry.origin} />
        <MemoryVersionMeta at={entry.updatedAt} rev={entry.rev} />
        <small><Autor rotulo={authorLabel(entry, (id) => host.authorName(id))} />{archived ? <> · {t('memoria.painel.arquivada')}</> : null}</small>
      </button>
      {entry.conflictText && <p className="memory-conflict-text selectable"><TextoMarcado chave="memoria.auto.conflito" texto={entry.conflictText} /></p>}
      <MemoryPrevious version={entry.previousVersion} name={(id) => host.bots.get(id)?.name} />
    </div>
    <DropdownMenu className="icon-button memory-menu" title={t('memoria.painel.acoes')} language={language} items={() => [
      { title: t('memoria.painel.editar'), icon: 'pencil', action: edit },
      { title: entry.pinned ? t('memoria.painel.desafixar') : t('memoria.painel.fixar'), icon: entry.pinned ? 'pin.slash' : 'pin', action: () => void memory.mutate('pinMemory', { id: entry.id, pinned: !entry.pinned }) },
      { title: archived ? t('memoria.painel.restaurar') : t('memoria.painel.arquivar'), icon: archived ? 'tray.and.arrow.up' : 'archivebox', action: () => void memory.mutate('archiveMemory', { id: entry.id, archived: !archived }) },
      { title: t('memoria.painel.esquecer'), icon: 'trash', destructive: true, divider: true, action: () => void memory.mutate('forgetMemory', { id: entry.id }) },
    ]}><Icon name="ellipsis" size={14} weight="medium" /></DropdownMenu>
  </div>
}
