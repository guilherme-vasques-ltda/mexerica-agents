import type { Entry } from '@shared/models'
import { Button, Spinner } from '../../components/Controls'
import { HStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStoreRef as useStore } from '../../store/context'
import { useSelector } from '../../lib/observable'
import { useT } from '../../lib/fork-i18n/estado'

/** Aviso de mudança do Berga: a restauração é validada e executada pelo host. */
export function TeamChangeNotice({ entry }: { entry: Entry }) {
  const store = useStore()
  const t = useT()
  const busy = useSelector(store, (s) => s.answering.has(entry.id))
  const available = entry.data.teamChange?.status === 'available'
  return (
    <HStack gap={8} justify="center">
      <Text size="footnote" tone="secondary">{available ? entry.data.text : t('conversa.avisos.equipe.desfeito', { texto: entry.data.text ?? '' })}</Text>
      {available && (busy ? <Spinner /> : (
        <Button kind="secondary" onClick={() => { void store.undoTeamChange(entry) }}>{t('conversa.avisos.equipe.desfazer')}</Button>
      ))}
    </HStack>
  )
}
