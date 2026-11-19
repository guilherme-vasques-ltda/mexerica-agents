import { useEffect, useMemo, useState } from 'react'
import type { BotDraft } from '@shared/models'
import { encodeTemplate } from '@shared/bot-template'
import { Icon } from '../../components/Icon'
import { ModalHeader } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { superficieIdioma, tradutor, useIdioma } from '../../lib/fork-i18n/estado'
import './bots.css'

/** A portable settings snapshot, with no conversation identity or runtime state. */
export function BotTemplateView({ draft }: { draft: BotDraft }) {
  const encoded = useMemo(() => encodeTemplate(draft), [draft])
  const [copied, setCopied] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)

  const copy = () => {
    void navigator.clipboard.writeText(encoded).then(() => setCopied(true))
  }

  // Return copies (the default action).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && !e.isComposing) {
        e.preventDefault()
        copy()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  return (
    <VStack className="bot-template idioma-superficie" {...superficieIdioma(idioma)}>
      <ModalHeader icon="square.on.square" title={t('bots.editor.modeloBot.titulo')} subtitle={draft.name} closeTitle={t('bots.editor.fechar')} />
      <VStack gap={16} className="bot-template-body">
        <Text size="body" tone="secondary">{t('bots.editor.modeloBot.descricao')}</Text>
        <div className="bot-template-code">
          {/* R1 do #572: o modelo é código; LTR e isolado mesmo num app em árabe. */}
          <pre className="selectable" dir="ltr" style={{ direction: 'ltr', unicodeBidi: 'isolate', textAlign: 'left' }}>
            {encoded}
          </pre>
        </div>
        <HStack gap={8}>
          <Text size="caption" tone="secondary" grow>{t('bots.editor.modeloBot.inclui')}</Text>
          <button className="press bot-template-copy" onClick={copy}>
            <Icon key={copied ? 'checkmark' : 'square.on.square'} name={copied ? 'checkmark' : 'square.on.square'} size={13} weight="semibold" className="fade-in" />
            {t(copied ? 'bots.editor.modeloBot.copiado' : 'bots.editor.modeloBot.copiar')}
          </button>
        </HStack>
      </VStack>
    </VStack>
  )
}
