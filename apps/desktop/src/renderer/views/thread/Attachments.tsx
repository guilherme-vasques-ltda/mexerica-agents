import { useEffect, useState } from 'react'
import { direcaoDoNome } from '@shared/bidi'
import { isImageName, type Attachment } from '@shared/models'
import { Icon } from '../../components/Icon'
import { ModalHeader, Sheet, SheetPage } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStoreRef } from '../../store/context'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import './attachments.css'

/** The symbol for a file by its extension. */
export function attachmentSymbol(name: string) {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'heic', 'gif', 'webp', 'tiff', 'bmp'].includes(ext)) return 'photo'
  if (ext === 'pdf') return 'doc.richtext'
  if (['zip', 'gz', 'tar', '7z'].includes(ext)) return 'doc.zipper'
  if (['mov', 'mp4', 'm4v'].includes(ext)) return 'film'
  if (['mp3', 'm4a', 'wav', 'aac'].includes(ext)) return 'waveform'
  return 'doc'
}

export function byteCount(bytes: number) {
  if (bytes < 1000) return `${bytes} bytes`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1000
  let unit = 0
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000
    unit++
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`
}

/** Files sent with a message: images as pictures (click for full size), other files as cards. */
export function AttachmentList({ attachments, botId }: { attachments: Attachment[]; botId: string }) {
  const [viewing, setViewing] = useState<Attachment | null>(null)
  const t = useT()
  const idioma = useIdioma()
  return (
    <VStack align="flex-end" gap={4} className="attachment-list">
      {attachments.map((file) =>
        isImageName(file.name) ? (
          <button key={file.id} className="attachment-image-button" aria-label={file.name} onClick={() => setViewing(file)}>
            <AttachmentImage file={file} botId={botId} fit="thumbnail" />
          </button>
        ) : (
          <HStack key={file.id} gap={8} className="attachment-file">
            <Icon name={attachmentSymbol(file.name)} size={16} color="var(--context-ink-2, var(--ink-2))" />
            <VStack gap={1} shrink>
              <Text size="subheadline" tone="text" truncate className="attachment-file-name" title={file.name}>{file.name}</Text>
              <Text size="caption2" tone="tertiary">{byteCount(file.size)}</Text>
            </VStack>
          </HStack>
        ),
      )}
      <Sheet language={superficieIdioma(idioma)} open={viewing !== null} onClose={() => setViewing(null)} width={720} height={560}>
        {viewing ? (
          <SheetPage>
            <ModalHeader closeTitle={t('conversa.fechar')} title={<bdi dir={direcaoDoNome(viewing.name)}>{viewing.name}</bdi>} />
            <div className="attachment-viewer">
              <AttachmentImage file={viewing} botId={botId} fit="full" />
            </div>
          </SheetPage>
        ) : null}
      </Sheet>
    </VStack>
  )
}

/** A sent image, fetched from the computer once and then read from the cache. */
function AttachmentImage({ file, botId, fit }: { file: Attachment; botId: string; fit: 'thumbnail' | 'full' }) {
  const store = useStoreRef()
  const [url, setUrl] = useState<string | null>(null)
  // Auditoria D249, F9: loading and "did not open" look different.
  const [failed, setFailed] = useState(false)
  const t = useT()
  useEffect(() => {
    let revoked = false
    let made: string | null = null
    setFailed(false)
    void store.attachmentData(file.id, botId).then((data) => {
      if (revoked) return
      if (!data) { setFailed(true); return }
      made = URL.createObjectURL(new Blob([data as BlobPart]))
      setUrl(made)
    })
    return () => {
      revoked = true
      if (made) URL.revokeObjectURL(made)
    }
  }, [store, file.id, botId])
  if (failed) {
    return (
      <div className="attachment-placeholder failed" role="img" aria-label={t('conversa.compositor.imagem.falhaAria', { arquivo: file.name })}>
        <Icon name="exclamationmark.triangle" size={16} color="var(--context-ink-2, var(--ink-2))" />
        <Text size="caption" tone="secondary">{t('conversa.compositor.imagem.falha')}</Text>
      </div>
    )
  }
  if (!url) {
    return (
      <div className="attachment-placeholder loading" role="img" aria-label={t('conversa.compositor.imagem.carregando', { arquivo: file.name })} aria-busy>
        <Icon name="photo" size={16} color="var(--context-ink-2, var(--ink-2))" />
        <Text size="caption" tone="secondary" truncate className="attachment-placeholder-name" title={file.name}>{file.name}</Text>
      </div>
    )
  }
  return <img src={url} alt="" className={`attachment-image ${fit}`} onError={() => setFailed(true)} />
}
