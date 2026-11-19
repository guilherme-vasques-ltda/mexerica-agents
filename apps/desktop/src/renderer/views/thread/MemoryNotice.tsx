import { memoryUndoable, migratedMemoryNote, type Entry } from '@shared/models'
import { Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { useModel } from '../../lib/observable'
import { useStore as useLiveStore } from '../../store/context'
import { MemoryNoticeStore } from '../../store/memory-store'
import { MemoryOrigin } from '../fork/memoria-auto/MemoryOrigin'
import { noticeText } from '../fork/memoria-auto/presentation'
import { memoryResponse } from '../fork/memoria-auto/placement'
import { MemoryNature, MemoryPrevious, MemoryVersionMeta } from '../fork/memoria-auto/MemoryNature'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from '../settings/rotulo'

// Conserva o resultado desconhecido ao sair e voltar à conversa, por Hub e recibo.
const notices = new WeakMap<object, MemoryNoticeStore>()

/** Linha persistente depois da resposta, inclusive para pendências legadas. */
export function MemoryNotice({ entry }: { entry: Entry }) {
  const host = useLiveStore()
  const t = useT()
  const idioma = useIdioma()
  let source = notices.get(host)
  if (!source) { source = new MemoryNoticeStore(() => host.ready()); notices.set(host, source) }
  const store = useModel(source)
  const state = store.state(entry)
  const migrated = !state.entry.data.memory ? migratedMemoryNote(state.entry.data) : undefined
  const memory = state.entry.data.memory ?? migrated
  if (!memory) return null
  const text = state.unknown ? {
    title: t('conversa.avisos.memoria.incerto'),
    detail: t('conversa.avisos.memoria.incertoDetalhe'),
  } : noticeText(t, memory)
  const undoable = memoryUndoable(memory)
  const response = memoryResponse(state.entry, host.allEntries(state.entry.botId))
  const name = (id: string) => host.bots.get(id)?.name
  // R1 do #597: o recibo declara o idioma e a direção do app (o corpo segue pt-BR/ltr até a última fatia).
  return <div className="memory-auto-notice idioma-superficie" {...superficieIdioma(idioma)} role="status" aria-live="polite">
    {!response && <p>{t('conversa.avisos.memoria.semOrigem')}</p>}
    <div className="memory-auto-line">
      <Icon name="brain" size={12} />
      <span className="memory-auto-title">{migrated ? t('conversa.avisos.memoria.nota', { titulo: text.title }) : text.title}</span>
      {!migrated && (undoable || state.unknown) && <>
        <span aria-hidden="true">·</span>
        {state.busy ? <><Spinner size={11} /><span>{t('conversa.avisos.memoria.desfazendo')}</span></>
          : <button className="press" onClick={() => void store.undo(state.entry)}>{t(state.unknown ? 'conversa.avisos.memoria.tentarDeNovo' : 'conversa.avisos.memoria.desfazer')}</button>}
      </>}
    </div>
    {memory.status === 'undone' && <p>{t('conversa.avisos.memoria.desfeita')}</p>}
    {memory.action !== 'note' && memory.text && <p className="memory-auto-fact">{memory.nature === 'inferred' ? comNos(t('conversa.avisos.memoria.hipotese', { texto: no('texto') }), { texto: <bdi dir="auto">{memory.text}</bdi> }) : memory.nature === 'conflict' ? comNos(t('conversa.avisos.memoria.anterior', { texto: no('texto') }), { texto: <bdi dir="auto">{memory.text}</bdi> }) : <bdi dir="auto">{memory.text}</bdi>}</p>}
    {memory.action !== 'note' && memory.conflictText && <p className="memory-auto-fact">{comNos(t('conversa.avisos.memoria.conflito', { texto: no('texto') }), { texto: <bdi dir="auto">{memory.conflictText}</bdi> })}</p>}
    <div className="memory-tags"><MemoryNature nature={memory.nature} bot={memory.natureBot} name={name} /><MemoryOrigin origin={memory.origin} /><MemoryVersionMeta at={memory.at} rev={memory.rev} /></div>
    {typeof memory.previous === 'string' && <MemoryPrevious name={name} version={{ text: memory.previous, origin: memory.previousOrigin, nature: memory.previousNature, natureBot: memory.previousNatureBot, conflictText: memory.previousConflictText, at: memory.previousAt, rev: memory.previousRev }} />}
    {text.detail && <p>{text.detail}</p>}
  </div>
}
