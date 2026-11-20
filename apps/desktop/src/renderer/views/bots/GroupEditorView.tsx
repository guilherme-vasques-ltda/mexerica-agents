// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useState } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { BotAvatar, GroupAvatar } from '../../components/Avatar'
import { IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader, SheetPage, useDismiss } from '../../components/Overlay'
import { VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { errorText } from './drafts'
import { AutoTextArea } from './parts'
import { superficieIdioma, tradutor, useIdioma, useT } from '../../lib/fork-i18n/estado'
import './bots.css'

/**
 * Change a group chat's name, what it's for and who's in it. Bots are added from a
 * search of the ones not in it yet. New groups start in `NewGroupView`.
 */
export function GroupEditorView({ group }: { group: Bot }) {
  const store = useStore()
  const dismiss = useDismiss()
  const [name, setName] = useState(group.name)
  const [about, setAbout] = useState(group.description)
  const [members, setMembers] = useState<string[]>(group.members)
  const [query, setQuery] = useState('')
  const [adding, setAdding] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<Bot | null>(null)
  const idioma = useIdioma()
  const t = tradutor(idioma)

  const picked = members.map((id) => store.bots.get(id)).filter((b): b is Bot => !!b)
  // Bots that can still join, filtered by the search.
  const q = query.trim().toLowerCase()
  const candidates = store.roster.filter((b) => !isGroup(b) && !members.includes(b.id) && (!q || b.name.toLowerCase().includes(q)))
  const canSave = members.length > 0 && !saving
  // "Alice, Bob" when no name is typed.
  const defaultName = picked.length ? picked.map((b) => b.name).join(', ') : t('bots.editor.nome')
  const current = store.bots.get(group.id)

  const toggle = (id: string) => {
    setMembers((list) => (list.includes(id) ? list.filter((m) => m !== id) : [...list, id]))
    setQuery('')
  }

  const save = () => {
    setSaving(true)
    setError(null)
    store
      .updateGroup({ id: group.id, kind: 'group', name: name.trim() || defaultName, description: about.trim(), members, pinned: current?.pinned })
      .then(dismiss)
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setSaving(false))
  }

  return (
    <SheetPage>
      <ModalHeader icon="person.2" title={t('bots.editor.canal.titulo')} trailing={saving ? <Spinner /> : <IconButton title={t('bots.editor.salvar')} icon="checkmark" disabled={!canSave} onClick={save} />} closeTitle={t('bots.editor.fechar')} />
      <div className="group-scroll idioma-superficie" {...superficieIdioma(idioma)}>
        <div className="group-form">
          <div className="group-identity">
            <GroupAvatar members={picked} size={84} />
            <input className="group-name" value={name} placeholder={defaultName} aria-label={t('bots.editor.nome')} onChange={(e) => setName(e.target.value)} />
            <AutoTextArea className="group-about" value={about} placeholder={t('bots.editor.canal.sobre')} minRows={1} maxRows={4} onChange={setAbout} />
          </div>
          <VStack>
            <div className="group-section-title">{t('bots.editor.canal.membros')}</div>
            {picked.map((bot) => (
              <div key={bot.id} className="member-row fade-in">
                <BotAvatar bot={bot} size={32} animated={false} />
                <span className="member-name">{bot.name}</span>
                <IconButton title={t('bots.editor.canal.remover', { bot: bot.name })} icon="xmark" onClick={() => toggle(bot.id)} />
              </div>
            ))}
            {adding ? (
              <>
                <input
                  className="field-box fade-in group-search"
                  autoFocus
                  value={query}
                  placeholder={t('bots.editor.canal.buscar')}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.nativeEvent.isComposing && candidates[0]) toggle(candidates[0].id)
                    if (e.key === 'Escape') setAdding(false)
                  }}
                />
                {candidates.map((bot) => (
                  <button key={bot.id} className="member-row press fade-in" aria-label={t('bots.editor.canal.adicionar', { bot: bot.name })} onClick={() => toggle(bot.id)}>
                    <BotAvatar bot={bot} size={32} animated={false} />
                    <Text tone="secondary" className="member-name">{bot.name}</Text>
                    <Icon name="plus.circle" size={18} color="var(--context-ink-2, var(--ink-2))" />
                  </button>
                ))}
              </>
            ) : candidates.length ? (
              <button className="member-row press" onClick={() => setAdding(true)}>
                <span className="member-add-icon">
                  <Icon name="plus" size={16} color="var(--context-ink-2, var(--ink-2))" />
                </span>
                <Text tone="secondary" className="member-name">{t('bots.editor.canal.adicionarMembro')}</Text>
              </button>
            ) : null}
            <Text as="div" size="caption" tone="tertiary" className="group-note">{t('bots.editor.canal.todosNaVez')}</Text>
          </VStack>
          {error ? <Text size="footnote" tone="danger" className="fade-in">{error}</Text> : null}
          {current ? (
            <button className="press group-delete" onClick={() => setConfirmDelete(current)}>
              {t('bots.editor.canal.apagar')}
            </button>
          ) : null}
        </div>
      </div>
      <DeleteBotDialog bot={confirmDelete} onClose={() => setConfirmDelete(null)} onDeleted={dismiss} />
    </SheetPage>
  )
}

/** The one "Delete <bot>?" confirmation (kit `deleteBotConfirmation`). */
function DeleteBotDialog({ bot, onClose, onDeleted }: { bot: Bot | null; onClose: () => void; onDeleted: () => void }) {
  const store = useStore()
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  return (
    <Dialog
      open={bot !== null}
      cancel={t('bots.editor.cancelar')}
      language={idioma}
      title={bot ? t('bots.editor.apagar.titulo', { bot: bot.name }) : t('bots.editor.apagar.tituloBot')}
      message={t(bot && isGroup(bot) ? 'bots.editor.apagar.canalFica' : 'bots.editor.apagar.arquivosFicam')}
      actions={[
        {
          title: t(bot && isGroup(bot) ? 'bots.editor.apagar.canal' : 'bots.editor.apagar.bot'),
          destructive: true,
          action: () => {
            if (bot) store.delete(bot)
            onDeleted()
          },
        },
      ]}
      onClose={onClose}
    />
  )
}
