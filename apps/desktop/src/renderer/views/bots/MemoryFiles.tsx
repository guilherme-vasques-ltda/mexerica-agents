// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useState, type ReactElement } from 'react'
import { CardForm, CardSection, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader } from '../../components/Overlay'
import { useApp, useStore } from '../../store/context'
import { type MemoryFile, type MemoryFileDetail, type MemoryFiles, type MemoryStore, versionLabel } from '../../store/memory-store'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../lib/fork-i18n/estado'
import { ltr } from '@shared/bidi'
import { Field } from '../marketplace/pieces'
import { MemoryOrigin } from '../fork/memoria-auto/MemoryOrigin'
import { Autor, ErroDaMemoria, MemoryNature, MemoryVersionMeta, TextoMarcado } from '../fork/memoria-auto/MemoryNature'

/** What a note sheet opens: a file, or (`name` null) a new one. */
export interface FileTarget { name: string | null; shared: boolean }

/** O modelo de uma nota nova, com a linha `description:` no idioma do app (o texto é da pessoa, que o edita). */
const modelo = (t: Tradutor) => `---\ndescription: ${t('memoria.painel.notas.modelo')}\n---\n`

/** One layer's long-form notes, as rows under its entries: name, description, size and date, marked past a cap. */
export function fileRows(t: Tradutor, memory: MemoryStore, files: MemoryFiles, shared: boolean, open: (target: FileTarget) => void): ReactElement[] {
  const folder = shared ? files.yours : files.bot
  const live = folder.files.filter((f) => !f.deleted)
  const gone = folder.files.filter((f) => f.deleted)
  return [
    <div key="notes" className="memory-notes-header">
      <span>{t('memoria.painel.notas')}</span>
      <IconButton title={t('memoria.painel.notas.nova')} icon="doc.badge.plus" disabled={memory.busy} onClick={() => open({ name: null, shared })} />
    </div>,
    ...(live.length === 0 ? [<p key="notes-empty" className="memory-muted">{t('memoria.painel.notas.vazio')}</p>] : []),
    ...(live.length > files.folderCap ? [<p key="notes-cap" className="memory-attention">{t.plural('memoria.painel.notas.acima', live.length, { limite: files.folderCap })}</p>] : []),
    ...[...live, ...gone].map((file) => <FileRow key={file.name} file={file} cap={files.cap} open={() => open({ name: file.name, shared })} />),
  ]
}

function FileRow({ file, cap, open }: { file: MemoryFile; cap: number; open: () => void }) {
  const host = useStore()
  const t = useT()
  const over = !file.deleted && file.tokens > cap
  return <button className="press memory-content fade-in" onClick={open}>
    <strong className={file.deleted ? 'memory-archived' : undefined}><Icon name={file.deleted ? 'doc' : 'doc.text'} size={11} /><bdi dir="ltr">{file.name}</bdi></strong>
    {file.description && !file.deleted ? <span className="memory-muted" dir="auto">{file.description}</span> : null}
    <MemoryOrigin origin={file.origin} />
    <MemoryNature nature={file.nature} bot={file.natureBot} name={(id) => host.bots.get(id)?.name} />
    {file.conflictText && <span><TextoMarcado chave="memoria.auto.conflito" texto={file.conflictText} /></span>}
    <small className={over ? 'memory-attention' : undefined}>
      {file.deleted ? t('memoria.painel.notas.excluida') : over ? t.plural('memoria.painel.notas.tokensAcima', file.tokens, { limite: cap }) : t.plural('memoria.painel.notas.tokens', file.tokens)}
    </small>
    <MemoryVersionMeta at={file.updatedAt} rev={file.rev} />
  </button>
}

/** One note: read it, edit it, open it in an editor, delete it, or bring back an earlier version. */
export function MemoryFileSheet({ memory, target, onClose }: { memory: MemoryStore; target: FileTarget; onClose: () => void }) {
  const host = useStore()
  const t = useT()
  const language = superficieIdioma(useIdioma())
  // A note opens in an editor only when the host is this computer.
  const local = useApp().local === host
  const [detail, setDetail] = useState<MemoryFileDetail | null>(null)
  const [draft, setDraft] = useState<string | null>(target.name ? null : modelo(t))
  const [name, setName] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  useEffect(() => {
    if (target.name) void memory.file(target.name, target.shared).then(setDetail)
  }, [memory, target])

  const validName = /^[a-z0-9-]+\.md$/.test(name)
  const change = async (change: { content?: string; version?: number }) => {
    const file = target.name ?? name
    const ok = await memory.changeFile(file, target.shared, target.name ? detail?.hash ?? null : null, change)
    if (!ok) return
    if (!target.name || (change.content === undefined && change.version === undefined)) return onClose()
    setDraft(null)
    setDetail(await memory.file(file, target.shared))
  }

  const actions = memory.busy ? <Spinner /> : draft !== null
    ? <IconButton title={t('memoria.painel.salvar')} icon="checkmark" disabled={!draft.trim() || (!target.name && !validName)} onClick={() => void change({ content: draft })} />
    : detail?.content != null ? <div className="memory-header-actions">
      <IconButton title={t('memoria.painel.editar')} icon="pencil" onClick={() => setDraft(detail.content)} />
      {local ? <IconButton title={t('memoria.painel.notas.abrirEditor')} icon="arrow.up.right.square" onClick={() => window.codync.app.openMemoryFile(detail.path)} /> : null}
      <IconButton title={t('memoria.painel.notas.excluir')} icon="trash" onClick={() => setConfirmDelete(true)} />
    </div> : null

  return <div className="memory-file-sheet">
    <ModalHeader closeTitle={t('memoria.painel.fechar')} title={target.name ? ltr(target.name) : t('memoria.painel.notas.nova')} trailing={actions} />
    <CardForm>
      {memory.error ? <CardSection><p role="alert" className="memory-error"><ErroDaMemoria m={memory.error} /></p></CardSection> : null}
      {draft !== null ? <>
        {!target.name ? <CardSection footer={t('memoria.painel.notas.nome.rodape')}>
          <Field value={name} onChange={setName} placeholder={t('memoria.painel.notas.nome.exemplo')} label={t('memoria.painel.notas.nome.campo')} mono />
        </CardSection> : null}
        <CardSection footer={t('memoria.painel.notas.nota.rodape')}>
          <Field value={draft} onChange={setDraft} label={t('memoria.painel.notas.nota.campo')} lines={[12, 32]} mono />
        </CardSection>
      </> : detail ? <>
        <CardSection footer={ltr(detail.path)}>
          {detail.content != null ? <pre className="memory-file-text" dir="auto">{detail.content}</pre> : <p className="memory-muted">{t('memoria.painel.notas.excluidaRestaure')}</p>}
        </CardSection>
        <CardSection title={t('memoria.painel.notas.versoes')}>
          {detail.versions.map((version) => <div key={version.id} className="memory-item">
            <div className="memory-content">
              <span><Autor rotulo={versionLabel(version, (id) => host.authorName(id))} /></span>
              <MemoryVersionMeta at={version.at} rev={version.rev} />
              <MemoryNature nature={version.nature} bot={version.natureBot} name={(id) => host.bots.get(id)?.name} />
              <MemoryOrigin origin={version.origin} />
              <details className="memory-previous"><summary>{t('memoria.painel.notas.verTexto')}</summary><pre className="memory-file-text">{version.content == null ? t('memoria.painel.notas.notaExcluida') : version.nature === 'inferred' ? <TextoMarcado chave="memoria.auto.hipotese" texto={version.content} /> : <bdi dir="auto">{version.content}</bdi>}</pre>{version.conflictText && <p><TextoMarcado chave="memoria.auto.conflito" texto={version.conflictText} /></p>}</details>
            </div>
            {version.content != null && version.content !== detail.content
              ? <IconButton title={t('memoria.painel.notas.restaurarVersao')} icon="arrow.uturn.backward" disabled={memory.busy} onClick={() => void change({ version: version.id })} />
              : null}
          </div>)}
        </CardSection>
      </> : <CardSection><p className="memory-muted">{t('memoria.painel.notas.carregando')}</p></CardSection>}
    </CardForm>
    <Dialog language={language} cancel={t('memoria.painel.cancelar')} open={confirmDelete} title={target.name ? t('memoria.painel.notas.excluirTitulo', { nota: ltr(target.name) }) : t('memoria.painel.notas.excluirEsta')} message={t('memoria.painel.notas.excluirMensagem')}
      actions={[{ title: t('memoria.painel.notas.excluir'), destructive: true, action: () => void change({}) }]} onClose={() => setConfirmDelete(false)} />
  </div>
}
