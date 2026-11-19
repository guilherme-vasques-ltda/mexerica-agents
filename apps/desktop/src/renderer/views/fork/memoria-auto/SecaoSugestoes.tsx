import { useState } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { MemoryStore } from '../../../store/memory-store'
import { useStore } from '../../../store/context'
import { panelDate } from '../bot-panel/panel-date'
import { MemoryNature } from './MemoryNature'
import { MemoryOrigin } from './MemoryOrigin'
import type { MemorySuggestion, SuggestionOutcome } from './sugestoes'
import './memory-auto.css'

const ACTION = { remember: 'memoria.sugestoes.acao.remember', revise: 'memoria.sugestoes.acao.revise', archive: 'memoria.sugestoes.acao.archive', confirm: 'memoria.sugestoes.acao.confirm', reclassify: 'memoria.sugestoes.acao.reclassify' } as const

/** D389: Sugestões na Memória do bot. Aprovar aplica como o modo automático (recibo e Desfazer); Descartar guarda. */
export function Sugestoes({ memory, items }: { memory: MemoryStore; items: MemorySuggestion[] }) {
  const t = useT()
  const idioma = useIdioma()
  const [outcome, setOutcome] = useState<{ id: string; value: SuggestionOutcome } | null>(null)
  const decide = async (id: string, approve: boolean) => {
    const value = await memory.decideSuggestion(id, approve)
    if (value) setOutcome({ id, value })
  }
  const message = outcome?.value === 'stale' ? t('memoria.sugestoes.mudou') : outcome?.value === 'noChange' ? t('memoria.sugestoes.semMudanca') : outcome?.value === 'error' ? t('memoria.sugestoes.erro') : null
  return <section aria-label={t('memoria.sugestoes.aba')} className="memory-suggestions idioma-superficie" {...superficieIdioma(idioma)}>
    <p className="memory-muted">{t('memoria.sugestoes.lead')}</p>
    {message && <p role={outcome?.value === 'error' ? 'alert' : 'status'} className="memory-muted">{message}</p>}
    {items.length === 0
      ? <div className="memory-empty"><Icon name="brain" size={20} color="var(--text-tertiary, var(--ink-3))" /><span>{t('memoria.sugestoes.vazio')}</span></div>
      : <ul className="memory-suggestion-list">{items.map((item) => <SuggestionRow key={item.id} item={item} bot={memory.botId} busy={memory.busy} decide={decide} />)}</ul>}
  </section>
}

function SuggestionRow({ item, bot, busy, decide }: { item: MemorySuggestion; bot: string; busy: boolean; decide: (id: string, approve: boolean) => Promise<void> }) {
  const t = useT()
  const host = useStore()
  const title = item.action === 'note' ? t('memoria.sugestoes.acao.note', { arquivo: item.file ?? '' }) : t(ACTION[item.action])
  return <li className="memory-item memory-suggestion fade-in">
    <div className="memory-content">
      <small>{title}{item.shared ? ` · ${t('memoria.sugestoes.compartilhada')}` : ''}{item.createdAt > 0 ? ` · ${panelDate(item.createdAt)}` : ''}</small>
      <strong className="selectable"><span className="line-clamp memory-clamp" title={item.text ?? undefined}>{item.text ?? t('memoria.sugestoes.removerNota')}</span></strong>
      <MemoryNature nature={item.nature} bot={item.nature === 'said' || item.nature === 'confirmed' ? null : bot} name={(id) => host.bots.get(id)?.name} />
      <MemoryOrigin origin={item.origin} />
    </div>
    <div className="memory-suggestion-actions">
      <Button kind="secondary" disabled={busy} onClick={() => void decide(item.id, false)}>{t('memoria.sugestoes.descartar')}</Button>
      <Button disabled={busy} onClick={() => void decide(item.id, true)}>{t('memoria.sugestoes.aprovar')}</Button>
    </div>
  </li>
}
