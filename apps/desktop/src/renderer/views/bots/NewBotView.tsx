import { useState, type ReactNode } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { BotAvatar, GroupAvatar } from '../../components/Avatar'
import { Button, ChoicePicker, Spinner } from '../../components/Controls'
import { ModalHeader, useDismiss } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { errorText } from './drafts'
import { superficieIdioma, tradutor, useIdioma, useT } from '../../lib/fork-i18n/estado'
import type { CreationAvailability } from '../../lib/creation-target'
import './bots.css'

/** Which computer a new bot or group goes to; shown when more than one is online. */
export interface ComputerChoice {
  options: { id: string; label: string }[]
  selection: string
  onChange: (id: string) => void
}

/** D133: a criação de bot é a tela no estilo do Estúdio (`views/criar-bot`). */
export { NovoBotView as NewBotView } from '../criar-bot/NovoBot'

/** A new group chat, laid out like a new bot: its face, an optional name, then the bots to put in it. */
export function NewGroupView({ computer, availability }: { computer: ComputerChoice | null; availability?: CreationAvailability }) {
  const store = useStore()
  const [name, setName] = useState('')
  const [members, setMembers] = useState<string[]>([])
  const t = useT()
  const bots = store.roster.filter((b) => !isGroup(b))
  const picked = members.map((id) => store.bots.get(id)).filter((b): b is Bot => !!b)
  // "Alice, Bob" when no name is typed.
  const defaultName = picked.map((b) => b.name).join(', ')

  const toggle = (id: string) => setMembers((list) => (list.includes(id) ? list.filter((m) => m !== id) : [...list, id]))
  const create = async () => {
    availability?.check()
    await store.createGroup(name.trim() || defaultName, '', members)
  }

  return (
    <CreateLayout
      title={t('bots.editor.canal.novo')}
      preview={<GroupAvatar members={picked} size={96} />}
      name={name}
      placeholder={defaultName || t('bots.editor.canal.nomePlaceholder')}
      onName={setName}
      canCreate={members.length > 0}
      computer={computer}
      availability={availability}
      create={create}
    >
      {bots.length ? (
        <HStack wrap justify="center" align="flex-start" gap={4} className="create-members">
          {bots.map((bot) => {
            const on = members.includes(bot.id)
            return (
              <button key={bot.id} className="press create-member" aria-pressed={on} onClick={() => toggle(bot.id)}>
                <span className={`swatch ${on ? 'on' : ''}`}>
                  <BotAvatar bot={bot} size={44} animated={false} />
                </span>
                <Text size="caption" tone={on ? 'text' : 'secondary'} truncate className="create-member-name">{bot.name}</Text>
              </button>
            )
          })}
        </HStack>
      ) : (
        <Text tone="secondary">{t('bots.editor.canal.semBots')}</Text>
      )}
      <Text as="div" size="caption" tone="tertiary">{t('bots.editor.canal.todosNaVez')}</Text>
    </CreateLayout>
  )
}

/** The create sheet both share: the face, a centered name field, the choices, then Cancel / Create. */
function CreateLayout({ title, preview, name, placeholder, onName, canCreate, computer, availability, create, children }: {
  title: string
  preview: ReactNode
  name: string
  placeholder: string
  onName: (name: string) => void
  canCreate: boolean
  computer: ComputerChoice | null
  availability?: CreationAvailability
  create: () => Promise<void>
  children: ReactNode
}) {
  const store = useStore()
  const dismiss = useDismiss()
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const ready = canCreate && !saving && !availability?.error && store.connection.kind === 'online'

  const submit = () => {
    if (!ready) return
    setSaving(true)
    setError(null)
    create()
      .then(dismiss)
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setSaving(false))
  }

  return (
    <VStack className="create-sheet idioma-superficie" {...superficieIdioma(idioma)}>
      <ModalHeader title={title} closeTitle={t('bots.editor.fechar')} />
      <VStack align="center" gap={20} className="create-body">
        {preview}
        <input
          className="field-box create-name"
          autoFocus
          value={name}
          placeholder={placeholder}
          aria-label={t('bots.editor.nome')}
          onChange={(e) => onName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) submit()
          }}
        />
        {children}
        {error || availability?.error ? <Text role="alert" size="footnote" tone="danger" className="fade-in">{error || availability?.error}</Text> : null}
      </VStack>
      <HStack gap={8} className="create-footer">
        {computer ? (
          <HStack gap={8}>
            <Text tone="secondary">{t('bots.editor.canal.computador')}</Text>
            <ChoicePicker language={superficieIdioma(idioma)} selection={computer.selection} options={computer.options} onChange={computer.onChange} />
          </HStack>
        ) : null}
        <Spacer />
        <Button kind="secondary" onClick={dismiss}>{t('bots.editor.cancelar')}</Button>
        <Button onClick={submit} disabled={!ready}>{saving ? <Spinner /> : t('bots.editor.criar')}</Button>
      </HStack>
    </VStack>
  )
}
