// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useRef } from 'react'
import { ChoicePicker, IconButton, Pill, Spinner } from '../../components/Controls'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import type { AgentModelsState } from './agent-models'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'

/** The bot's model and its options, from the list `useAgentModels` loaded for its agent. */
export function AgentModelPicker({ backend, models, selection, onChange, options: chosen, onOptionsChange }: {
  backend: string
  models: AgentModelsState
  selection: string | null | undefined
  onChange: (model: string | null) => void
  options: Record<string, string>
  onOptionsChange: (options: Record<string, string>) => void
}) {
  const { catalog, loading, failure } = models
  const t = useT()
  const superficie = superficieIdioma(useIdioma())
  const custom = backend === 'custom'

  // A different agent starts on its own default model.
  const lastBackend = useRef(backend)
  useEffect(() => {
    if (lastBackend.current === backend) return
    lastBackend.current = backend
    onChange(null)
  }, [backend, onChange])

  // The agent's own "Default (recommended)" entry is the same choice as leaving the model unset,
  // so it folds into the single Default row instead of showing twice.
  const agentDefault = catalog?.models.find((m) => m.name.toLowerCase().startsWith('default'))
  const available = (catalog?.models ?? []).filter((m) => m.id !== agentDefault?.id)
  const current = available.find((m) => m.id === catalog?.currentModelId)?.name
  const options = [{ id: '', label: agentDefault?.name ?? (current ? t('bots.editor.modelo.padraoCom', { modelo: current }) : t('bots.editor.modelo.padrao')) }, ...available.map((m) => ({ id: m.id, label: m.name }))]
  if (selection && selection !== agentDefault?.id && !available.some((m) => m.id === selection)) options.push({ id: selection, label: selection })
  const value = selection === agentDefault?.id ? '' : (selection ?? '')
  const set = (v: string) => onChange(v ? v : null)
  const empty = catalog?.models.length === 0
  const modelOptions = available.find((m) => m.id === value)?.options ?? []
  const setOption = (id: string, v: string) => {
    const { [id]: _, ...rest } = chosen
    onOptionsChange(v ? { ...rest, [id]: v } : rest)
  }

  return (
    <VStack gap={6}>
      <HStack gap={8}>
        <Text tone="text" className="model-picker-label">{t('bots.editor.modelo.rotulo')}</Text>
        <Spacer />
        <HStack gap={4} shrink>
          {custom ? null : (
            // Loading and refresh share one fixed slot, so the row never grows for loading.
            <div className="model-picker-slot">
              <div className={`model-picker-refresh ${loading ? 'hidden' : ''}`}>
                <IconButton title={t(failure ? 'bots.editor.modelo.tentarDeNovo' : 'bots.editor.modelo.atualizar')} icon="arrow.clockwise" disabled={loading} onClick={models.reload} />
              </div>
              {loading ? (
                <HStack align="stretch" className="fade-in" aria-label={t('bots.editor.modelo.carregando')}>
                  <Spinner size={12} />
                </HStack>
              ) : null}
            </div>
          )}
          {custom || (!loading && (!catalog || empty)) ? (
            <Pill outlined className="model-picker-pill">
              <input className="model-picker-input" value={value} placeholder={t('bots.editor.modelo.padrao')} spellCheck={false} onChange={(e) => set(e.target.value)} />
            </Pill>
          ) : (
            <HStack align="stretch" shrink>
              <ChoicePicker language={superficie} selection={value} options={options} onChange={set} />
            </HStack>
          )}
        </HStack>
      </HStack>
      {modelOptions.map((o) => {
        const fallback = o.values.find((v) => v.id === o.default)?.name
        const choices = [{ id: '', label: fallback ? t('bots.editor.modelo.padraoCom', { modelo: fallback }) : t('bots.editor.modelo.padrao') }, ...o.values.map((v) => ({ id: v.id, label: v.name }))]
        return (
          <HStack key={o.id} gap={8} className="fade-in">
            <Text tone="text" className="model-picker-label">{o.name}</Text>
            <Spacer />
            <ChoicePicker language={superficie} selection={chosen[o.id] ?? ''} options={choices} onChange={(v) => setOption(o.id, v)} />
          </HStack>
        )
      })}
      {!custom && !loading && failure ? (
        <Text size="caption" tone="attention">{failure}</Text>
      ) : !custom && !loading && empty ? (
        <Text size="caption" tone="secondary">{t('bots.editor.modelo.semLista')}</Text>
      ) : null}
      <Text size="caption" tone="secondary">{t('bots.editor.modelo.trocar')}</Text>
    </VStack>
  )
}
