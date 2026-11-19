import type { Entry, SharedFile } from '@shared/models'
import { useStore } from '../../store/context'
import { Icon } from '../../components/Icon'
import { IconButtonBase } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { attachmentSymbol, byteCount } from './Attachments'
import { useT } from '../../lib/fork-i18n/estado'
import './attachments.css'

export function FileDownloadCard({ entry, file }: { entry: Entry; file: SharedFile }) {
  const store = useStore()
  const state = store.fileDownloads.states.get(file.id)
  const busy = state?.kind === 'downloading'
  const t = useT()
  const action = t(busy ? 'conversa.mensagens.arquivo.cancelar' : state?.kind === 'failed' ? 'conversa.mensagens.arquivo.tentarDeNovo' : state?.kind === 'saved' ? 'conversa.mensagens.arquivo.salvarDeNovo' : 'conversa.mensagens.arquivo.baixar')
  const detail = busy ? t('conversa.mensagens.arquivo.progresso', { recebido: byteCount(state.received), total: byteCount(file.size) }) : state?.kind === 'failed' ? state.message : state?.kind === 'saved' ? t('conversa.mensagens.arquivo.salvo', { tamanho: byteCount(file.size) }) : byteCount(file.size)
  return <HStack gap={12} className="file-card">
    <Icon name={attachmentSymbol(file.name)} size={24} color="var(--context-ink-2, var(--ink-2))" />
    <VStack grow>
      <Text as="div" size="subheadline" weight="semibold" truncate title={file.name}>{file.name}</Text>
      <Text as="div" size="caption2" tone={state?.kind === 'failed' ? 'danger' : 'secondary'} role={state?.kind === 'failed' ? 'alert' : 'status'} className="file-card-detail">{detail}</Text>
    </VStack>
    <IconButtonBase title={action} icon={busy ? 'xmark' : 'arrow.down.to.line'} size={36} onClick={() => busy ? store.fileDownloads.cancel(file.id) : void store.fileDownloads.save(store.client, entry.id, file)} />
  </HStack>
}
