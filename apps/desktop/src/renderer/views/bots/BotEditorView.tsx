// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { avatarDoBot } from '@shared/fork-avatar-config'
import { botEditorUpdate, chooseNewBotPermission } from '@shared/bot-autonomy'
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { fastModeRefusal, fastModeShown } from '@shared/fast-mode'
import { showsComputerAccess } from '@shared/fork-features'
import { draftOf, type Bot, type BotDraft, type DirListing } from '@shared/models'
import { AgentIcon } from '../../components/AgentIcon'
import { DonnabotAvatar } from '../../fork/avatars/Avatar'
import { ProfileAvatarEditor } from '../../fork/avatars/Editors'
import { CardSection, ChoicePicker, DropdownMenu, IconButton, Pill, SearchField, Spinner, ValueRow } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, Sheet, SheetPage, useDismiss } from '../../components/Overlay'
import { HStack, IconSlot, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { useAgentModels } from './agent-models'
import { AgentModelPicker } from './AgentModelPicker'
import { errorText, isValid, lastPathComponent, normalized, useFollowSaved, usePlugins, type Plugins } from './drafts'
import { loginTitle } from '../marketplace/market-models'
import { FormSheet } from '../marketplace/pieces'
import { SkillSheet, type SkillRef } from '../marketplace/SkillSheet'
import { origemDaFonte } from '../fork/catalogo-skills/consumidores'
import { captureSkillChoice } from '../../store/fork-bot-settings-intents'
import { AnchoredPanel, AutoTextArea, Field, SwitchRow } from './parts'
import './bots.css'
import { traduzir } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from '../settings/rotulo'
import { textoDaMensagem } from '../fork/bot-settings/mensagem'

/** Edit a bot in a modal: the settings form plus Close / Save. New bots start in `NewBotView`. */
export function BotEditorView({ draft: initial }: { draft: BotDraft }) {
  const store = useStore()
  const t = useT()
  const dismiss = useDismiss()
  const plugins = usePlugins(store)
  const [draft, setDraft] = useState(initial)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const online = store.connection.kind === 'online'
  useFollowSaved(initial.id ? store.bots.get(initial.id) : undefined, setDraft)

  const save = () => {
    if (!isValid(draft) || saving || !online) return
    setSaving(true)
    setError(null)
    store
      .save(normalized(draft))
      .then(dismiss)
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setSaving(false))
  }

  return (
    <SheetPage>
      <ModalHeader
        title={t('bots.editor.ajustes')}
        trailing={saving ? <Spinner /> : <IconButton title={t('bots.editor.salvar')} icon="checkmark" disabled={!isValid(draft) || saving || !online} onClick={save} />}
        closeTitle={t('bots.editor.fechar')}
      />
      <div className="bot-editor-body">
        <BotSettingsForm draft={draft} onChange={setDraft} error={error} plugins={plugins} />
      </div>
    </SheetPage>
  )
}

/**
 * Which settings a form shows: all of them (the editor sheet), what changes often
 * (instructions, model, fast mode: the inspector's Details tab) or the rest (its Settings tab).
 */
export type FormPart = 'all' | 'often' | 'rarely'

/** The desktop inspector next to a conversation: the same form, saved as you edit. */
export function BotSettingsPanel({ botId, part }: { botId: string; part: FormPart }) {
  const store = useStore()
  const plugins = usePlugins(store)
  const [draft, setDraft] = useState<BotDraft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const change = useCallback((update: (d: BotDraft) => BotDraft) => setDraft((d) => (d ? update(d) : d)), [])

  useEffect(() => {
    const bot = store.bots.get(botId)
    setDraft(bot ? draftOf(bot) : null)
    // Only a different bot reloads the form; the host's echo of a save must not undo typing.
  }, [botId])
  useFollowSaved(store.bots.get(botId), change)

  // Autosave a moment after the last change.
  useEffect(() => {
    const bot = store.bots.get(botId)
    if (!draft || !bot || JSON.stringify(botEditorUpdate(draft)) === JSON.stringify(botEditorUpdate(draftOf(bot))) || !isValid(draft)) return
    const timer = setTimeout(() => {
      store
        .save(normalized(draft))
        .then(() => setError(null))
        .catch((e: unknown) => setError(errorText(e)))
    }, 600)
    return () => clearTimeout(timer)
  }, [draft])

  return draft ? <BotSettingsForm draft={draft} onChange={change} error={error} plugins={plugins} part={part} /> : null
}

/** The bot that created this one with its team tools, and when. */
function CreatedBy({ bot, creator }: { bot: Bot; creator: string }) {
  const store = useStore()
  const t = useT()
  const date = new Date(bot.createdAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
  return (
    <ValueRow label={t('bots.editor.criadoPor')}>
      <Text truncate>{comNos(t('bots.editor.criadoPorData', { bot: no('bot'), data: date }), { bot: <bdi dir="auto">{store.bots.get(creator)?.name ?? t('bots.editor.umBot')}</bdi> })}</Text>
    </ValueRow>
  )
}

/** Escolha do usuário, independente do rascunho e do botão Salvar. */
function PrimaryBotControl({ bot }: { bot: Bot }) {
  const store = useStore()
  const t = useT()
  const online = store.connection.kind === 'online'
  const [primaryBotId, setPrimaryBotId] = useState<string | null | undefined>()
  const [changing, setChanging] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    setPrimaryBotId(undefined)
    if (online) void store.teamSettings()
      .then((settings) => { if (live) { setPrimaryBotId(settings.primaryBotId); setError(null) } })
      .catch((e: unknown) => { if (live) setError(errorText(e)) })
    return () => { live = false }
  }, [store, bot.id, bot.manageTeam, online])

  return (
    <div>
      <SwitchRow
        title={t('bots.editor.principal')}
        detail={t('bots.editor.principalDetalhe')}
        on={primaryBotId === bot.id}
        disabled={!online || changing || primaryBotId === undefined}
        onChange={(on) => {
          setChanging(true)
          setError(null)
          void store.setPrimaryBot(on ? bot.id : null)
            .then((settings) => setPrimaryBotId(settings.primaryBotId))
            .catch((e: unknown) => setError(errorText(e)))
            .finally(() => setChanging(false))
        }}
      />
      {error ? <Text size="footnote" tone="danger">{error}</Text> : null}
    </div>
  )
}

/** Bot settings, in sections in the Settings style (`CardSection`). */
function BotSettingsForm({ draft, onChange, error, plugins, part = 'all' }: {
  draft: BotDraft
  onChange: (update: (d: BotDraft) => BotDraft) => void
  error: string | null
  plugins: Plugins | null
  part?: FormPart
}) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const idiomaApp = useIdioma()
  const [pickingAvatar, setPickingAvatar] = useState(false)
  const [pickingFolder, setPickingFolder] = useState(false)
  const [viewingSkill, setViewingSkill] = useState<SkillRef | null>(null)
  const [permissionSaving, setPermissionSaving] = useState(false)
  const [permissionError, setPermissionError] = useState<string | null>(null)
  const [skillSaving, setSkillSaving] = useState(false)
  const [skillError, setSkillError] = useState<string | null>(null)
  const avatarRef = useRef<HTMLButtonElement>(null)
  const set = <K extends keyof BotDraft>(key: K, value: BotDraft[K]) => onChange((d) => ({ ...d, [key]: value }))
  // Options belong to one model.
  const setModel = useCallback((model: string | null) => onChange((d) => (d.model === model ? d : { ...d, model, modelOptions: {} })), [onChange])
  const setModelOptions = useCallback((modelOptions: Record<string, string>) => onChange((d) => ({ ...d, modelOptions })), [onChange])
  const models = useAgentModels(draft.backend)
  const saved = draft.id ? (store.bots.get(draft.id) ?? null) : null
  const choosePermission = (permission: string) => {
    if (!saved) { onChange((d) => chooseNewBotPermission(d, permission)); return }
    setPermissionSaving(true)
    setPermissionError(null)
    void store.setPermission(saved.id, permission)
      .then((bot) => set('permission', bot.permission))
      .catch((e: unknown) => setPermissionError(errorText(e)))
      .finally(() => setPermissionSaving(false))
  }

  const backends = store.hello?.backends ?? []
  const backend = backends.find((b) => b.id === draft.backend)
  const connectors = plugins?.connectors ?? []
  const skills = plugins?.skills ?? []
  const logins = plugins?.logins ?? []
  // c1: cada skill é uma intenção própria, capturada no gesto; o autosave do perfil não a leva.
  const toggleSkill = (id: string, on: boolean) => {
    if (skillSaving) return
    const skill = skills.find((s) => s.id === id)
    const choice = saved && skill ? captureSkillChoice(store, saved, skill, on) : { refused: { chave: 'bots.editor.skillsDepois' } as const }
    if ('refused' in choice) { setSkillError(textoDaMensagem(t, choice.refused)); return }
    setSkillSaving(true)
    setSkillError(null)
    void choice.send().catch((e: unknown) => setSkillError(errorText(e))).finally(() => setSkillSaving(false))
  }

  const instructions = (
    <Field label={t('bots.editor.instrucoesFixas')} detail={t('bots.editor.instrucoesFixasDetalhe')}>
      <AutoTextArea className="field-box" value={draft.description} placeholder={t('bots.editor.instrucoesExemplo')} minRows={3} maxRows={8} onChange={(v) => set('description', v)} />
    </Field>
  )
  const model = [
    <AgentModelPicker key="model" backend={draft.backend} models={models} selection={draft.model} onChange={setModel} options={draft.modelOptions ?? {}} onOptionsChange={setModelOptions} />,
    (draft.backend !== 'custom' && models.catalog?.fastMode) || draft.fastMode || (saved?.backend === draft.backend && fastModeShown(saved)) ? (
      // Shown while the agent offers it (its model list, or the bot's own sessions), or while it's on.
      <div key="fast" className="fade-in">
        <SwitchRow
          title={t('bots.editor.fast')}
          detail={(saved && fastModeRefusal(t, saved)) ?? t('bots.editor.fastDetalhe')}
          on={draft.fastMode ?? false}
          onChange={(on) => set('fastMode', on)}
        />
      </div>
    ) : null,
  ]

  const agent = (
    <VStack key="agent" gap={8}>
      <ValueRow label={t('bots.editor.motor')}>
        <ChoicePicker
          language={superficieIdioma(idiomaApp)}
          selection={draft.backend}
          options={[
            ...backends.map((b) => ({ id: b.id, label: b.available ? b.name : t('bots.editor.naoInstalado', { motor: b.name }), leading: <AgentIcon registry={b.registry ?? b.id} size={14} /> })),
            { id: 'custom', label: t('bots.editor.comandoProprio'), leading: <AgentIcon registry="custom" size={14} /> },
          ]}
          onChange={(id) => set('backend', id)}
        />
      </ValueRow>
      {backend && !backend.available ? <Text size="caption" tone="attention">{backend.installHint}</Text> : null}
      {draft.backend === 'custom' ? (
        <input
          className="field-box fade-in bot-command"
          value={draft.command ?? ''}
          placeholder={t('bots.editor.comandoExemplo')}
          spellCheck={false}
          onChange={(e) => set('command', e.target.value || null)}
        />
      ) : null}
    </VStack>
  )

  if (part === 'often') {
    return (
      <div className="bot-form">
        <div className="bot-form-content">
          <CardSection title={t('bots.editor.instrucoes')}>{instructions}</CardSection>
          <CardSection title={t('bots.editor.motor')}>
            {agent}
            {model}
          </CardSection>
          {error ? <Text size="footnote" tone="danger" className="fade-in">{error}</Text> : null}
        </div>
      </div>
    )
  }

  return (
    <div className="bot-form">
      <div className="bot-form-content">
        <CardSection title={t('bots.editor.perfil')}>
          <ValueRow label={t('bots.editor.avatar')}>
            <button ref={avatarRef} className="avatar-button small" title={t('bots.editor.editarAvatar')} aria-label={t('bots.editor.editarAvatar')} onClick={() => setPickingAvatar((p) => !p)}>
              <DonnabotAvatar config={avatarDoBot(draft)} size={32} label={draft.name} />
            </button>
            <AnchoredPanel open={pickingAvatar} onClose={() => setPickingAvatar(false)} anchor={avatarRef}>
              <div className="avatar-picker-panel">
                <ProfileAvatarEditor key={draft.id} store={store} draft={draft} onChange={onChange} />
              </div>
            </AnchoredPanel>
          </ValueRow>
          <Field label={t('bots.editor.nome')}>
            <input className="field-box" value={draft.name} placeholder={t('bots.editor.nome')} onChange={(e) => set('name', e.target.value)} />
          </Field>
          {part === 'all' ? instructions : null}
          {/* Only a row when a bot made it: CardSection gives every child a row. */}
          {saved?.createdBy ? <CreatedBy bot={saved} creator={saved.createdBy} /> : null}
        </CardSection>

        {/* In the inspector the agent and its model are on the Details tab; what's left is where it works. */}
        <CardSection title={t(part === 'all' ? 'bots.editor.motor' : 'bots.editor.espaco')}>
          <ValueRow label={t('bots.editor.computador')}>
            <Text truncate>{store.hostName}</Text>
          </ValueRow>
          {part === 'all' ? agent : null}
          {part === 'all' ? model : null}
          <ValueRow
            label={t('bots.editor.espaco')}
            detail={
              draft.cwd
                ? t('bots.editor.espacoProjeto')
                : t('bots.editor.espacoProprio')
            }
          >
            <DropdownMenu
              language={superficieIdioma(idiomaApp)}
              title={draft.cwd || t('bots.editor.espacoPersistente')}
              items={() => [
                { title: t('bots.editor.espacoPessoal'), selected: !draft.cwd, action: () => set('cwd', '') },
                { title: t('bots.editor.escolherPasta'), selected: !!draft.cwd, action: () => setPickingFolder(true) },
              ]}
            >
              <Pill outlined className="workspace-pill">
                <Text truncate className="workspace-pill-label">{draft.cwd ? lastPathComponent(draft.cwd) : t('bots.editor.pessoal')}</Text>
                <Icon name="chevron.down" size={10} weight="semibold" color="var(--context-ink-2, var(--ink-2))" />
              </Pill>
            </DropdownMenu>
          </ValueRow>
          {saved?.workspaceIssue ? <Text size="footnote" tone="attention">{saved.workspaceIssue}</Text> : null}
          <ValueRow
            label={t('bots.editor.autonomia')}
            detail={
              draft.permission === 'auto'
                ? t('bots.editor.autonomiaLiberar')
                : t('bots.editor.autonomiaPedir')
            }
          >
            <ChoicePicker
          language={superficieIdioma(idiomaApp)}
              selection={draft.permission}
              options={[
                { id: 'ask', label: t('bots.editor.pedir') },
                { id: 'auto', label: t('bots.editor.liberar') },
              ]}
              disabled={permissionSaving || store.connection.kind !== 'online'}
              onChange={choosePermission}
            />
          </ValueRow>
          {permissionError ? <Text size="footnote" tone="danger">{permissionError}</Text> : null}
        </CardSection>

        <CardSection title={t('bots.editor.atividade')}>
          <SwitchRow title={t('bots.editor.notificacoes')} detail={t('bots.editor.notificacoesDetalhe')} on={draft.notify ?? true} onChange={(on) => set('notify', on)} />
          {/* Fork: o computador do bot é o Hub, sem tela; sem host local (D76) o interruptor e a dica ficam escondidos (showsComputerAccess). */}
          {store.screen && part === 'all' && showsComputerAccess() && !store.privateMacScreen ? (
            <SwitchRow
              title={t('bots.editor.usarComputador')}
              detail={
                store.screen.platform !== 'windows' && !(store.screen.enabled && store.screen.connected && store.screen.capture && store.screen.input)
                  ? traduzir('pt-BR', 'marca.bot.appsPermissoes')
                  : store.screen.enabled
                  ? t('bots.editor.usarComputadorCelular')
                  : store.screen.computerUse
                    ? t('bots.editor.usarComputadorDetalhe')
                    : traduzir('pt-BR', 'marca.bot.telaPermissoes')
              }
              on={draft.computer ?? false}
              onChange={(on) => set('computer', on)}
            />
          ) : null}
          {saved ? <PrimaryBotControl key={saved.id} bot={saved} /> : null}
        </CardSection>

        <PluginToggles
          title={t('bots.editor.conectores')}
          footer={t('bots.editor.conectoresRodape')}
          empty={t('bots.editor.conectoresVazio')}
          items={connectors.map((c) => ({ id: c.id, name: c.name, detail: c.command ?? c.url ?? '' }))}
          selection={draft.connectors ?? []}
          onChange={(ids) => set('connectors', ids)}
        />
        <PluginToggles
          title={t('bots.editor.logins')}
          empty={t('bots.editor.loginsVazio')}
          footer={t('bots.editor.loginsRodape')}
          items={logins.map((l) => ({ id: l.id, name: loginTitle(l), detail: l.username ? `${l.site} · ${l.username}` : l.site }))}
          selection={draft.logins ?? []}
          onChange={(ids) => set('logins', ids)}
        />
        <PluginToggles
          title={t('bots.editor.skills')}
          empty={t('bots.editor.skillsVazio')}
          items={skills.map((s) => ({ id: s.id, name: s.name, detail: s.description }))}
          selection={saved?.skills ?? draft.skills ?? []}
          onChange={() => {}}
          onToggle={toggleSkill}
          onInfo={(s) => setViewingSkill({ id: s.id, name: s.name, description: s.detail, origem: origemDaFonte(store) })}
        />
        {skillError ? <Text size="footnote" tone="danger" className="fade-in">{skillError}</Text> : null}
        {error ? <Text size="footnote" tone="danger" className="fade-in">{error}</Text> : null}
      </div>
      <FormSheet language={superficieIdioma(idioma)} open={viewingSkill !== null} onClose={() => setViewingSkill(null)}>
        {viewingSkill ? <SkillSheet key={viewingSkill.name} skill={viewingSkill} /> : null}
      </FormSheet>
      <Sheet language={superficieIdioma(idioma)} open={pickingFolder} onClose={() => setPickingFolder(false)} width={460} height={520}>
        <FolderPicker
          start={draft.cwd || store.hello?.home || null}
          onPick={(path) => {
            set('cwd', path)
            setPickingFolder(false)
          }}
        />
      </Sheet>
    </div>
  )
}

/** A card of on/off switches for the connectors, skills or sign-ins on the computer. */
function PluginToggles({ title, empty, footer, accessory, items, selection, onChange, onToggle, onInfo }: {
  title: string
  empty: string
  footer?: string
  /** Beside the title (fork R3: the skills index size). */
  accessory?: ReactNode
  items: { id: string; name: string; detail: string; icon?: ReactNode }[]
  selection: string[]
  onChange: (ids: string[]) => void
  /** Uma célula por gesto (skills, c1); quando presente, substitui `onChange`. */
  onToggle?: (id: string, on: boolean) => void
  /** Shows an item's details from an info button beside its switch. */
  onInfo?: (item: { id: string; name: string; detail: string }) => void
}) {
  const t = useT()
  return (
    <CardSection title={title} footer={items.length ? footer : undefined} accessory={accessory}>
      {items.length ? null : <Text tone="secondary">{empty}</Text>}
      {items.map((item) => {
        const row = (
          <SwitchRow
            key={item.id}
            title={item.name}
            detail={item.detail || null}
            icon={item.icon}
            on={selection.includes(item.id)}
            onChange={(on) => onToggle ? onToggle(item.id, on) : onChange([...selection.filter((id) => id !== item.id), ...(on ? [item.id] : [])])}
          />
        )
        return onInfo ? (
          <HStack key={item.id} gap={4}>
            <div className="plugin-toggle">{row}</div>
            <IconButton title={t('bots.editor.sobre', { nome: item.name })} icon="info.circle" onClick={() => onInfo(item)} />
          </HStack>
        ) : row
      })}
    </CardSection>
  )
}

/** Browses folders on the host, drilling down in place. */
function FolderPicker({ start, onPick }: { start: string | null; onPick: (path: string) => void }) {
  // The folders opened so far; the last one is shown.
  const [stack, setStack] = useState<(string | null)[]>([start])
  const [forward, setForward] = useState(true)
  const current = stack[stack.length - 1] ?? null
  const t = useT()
  const idioma = useIdioma()
  // R1 do #572: o seletor abre numa folha (portal no body): a raiz declara o idioma e a direção da tela.
  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <HStack>
        {stack.length > 1 ? (
          <HStack align="stretch" className="fade-in folder-back">
            <IconButton
              title={t('bots.editor.voltar')}
              icon="chevron.left"
              onClick={() => {
                setForward(false)
                setStack((s) => s.slice(0, -1))
              }}
            />
          </HStack>
        ) : null}
        <div className="folder-title">
          <ModalHeader title={current ? lastPathComponent(current) : t('bots.editor.pastas')} closeTitle={t('bots.editor.fechar')} />
        </div>
      </HStack>
      <div className="folder-levels">
        <FolderLevel
          key={stack.length}
          path={current}
          forward={forward}
          onPick={onPick}
          onOpen={(path) => {
            setForward(true)
            setStack((s) => [...s, path])
          }}
        />
      </div>
      </div>
    </SheetPage>
  )
}

/** One folder's subfolders. */
function FolderLevel({ path, forward, onPick, onOpen }: { path: string | null; forward: boolean; onPick: (path: string) => void; onOpen: (path: string) => void }) {
  const store = useStore()
  const t = useT()
  const [listing, setListing] = useState<DirListing | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [filter, setFilter] = useState('')

  useEffect(() => {
    let live = true
    store
      .listDirs(path)
      .then((l) => live && setListing(l))
      .catch((e: unknown) => live && setError(errorText(e)))
    return () => {
      live = false
    }
  }, [store, path])

  const dirs = listing?.dirs.filter((d) => !filter || d.name.toLowerCase().includes(filter.toLowerCase())) ?? []

  return (
    <div className={`bot-form level-slide ${forward ? 'from-trailing' : 'from-leading'}`}>
      <div className="bot-form-content folder-level-content">
        <SearchField value={filter} onChange={setFilter} />
        {listing ? (
          <>
            <CardSection footer={listing.path}>
              <button className="press folder-use" onClick={() => onPick(listing.path)}>
                <Icon name="checkmark.circle.fill" size={13} />
                <Text truncate>{t('bots.editor.usarPasta', { pasta: lastPathComponent(listing.path) })}</Text>
              </button>
            </CardSection>
            {dirs.length ? (
              <CardSection>
                {dirs.map((dir) => (
                  <button key={dir.path} className="press folder-row" onClick={() => onOpen(dir.path)}>
                    <IconSlot width={20}>
                      <Icon name={dir.isGit ? 'arrow.triangle.branch' : 'folder'} size={12} color={dir.isGit ? 'var(--accent)' : 'var(--context-ink-2, var(--ink-2))'} />
                    </IconSlot>
                    <Text tone="text" truncate grow>{dir.name}</Text>
                    <Icon name="chevron.right" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />
                  </button>
                ))}
              </CardSection>
            ) : null}
          </>
        ) : error ? (
          <Text tone="danger">{error}</Text>
        ) : (
          <HStack align="stretch" justify="center">
            <Spinner size={20} />
          </HStack>
        )}
      </div>
    </div>
  )
}

export { FolderPicker, PluginToggles }
