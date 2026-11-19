import { useEffect, useState, type ReactNode } from 'react'
import { CardSection, IconButton } from '../../components/Controls'
import { Dialog } from '../../components/Overlay'
import { VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { superficieIdioma, useT, useIdioma } from '../../lib/fork-i18n/estado'
import { errorText, rawError, type RoutineWebhook } from './routine'
import { LabeledRow } from './RoutineScheduleFields'
import './routines.css'

/** A saved webhook routine's address and key: copy either, reveal or replace the key. */
export function RoutineWebhookPanel({ botId, routineId }: { botId: string; routineId: string }) {
  const store = useStore()
  const idioma = useIdioma()
  const t = useT()
  const [webhook, setWebhook] = useState<RoutineWebhook | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showKey, setShowKey] = useState(false)
  const [copied, setCopied] = useState<'url' | 'key' | null>(null)
  const [confirmRotate, setConfirmRotate] = useState(false)
  const [rotating, setRotating] = useState(false)

  const load = async (rotate: boolean) => {
    setRotating(rotate)
    try {
      const value = await (await store.ready()).call<RoutineWebhook>('routineWebhook', { botId, id: routineId, rotate })
      setWebhook(value)
      if (rotate) {
        setCopied(null)
        setShowKey(true)
      }
      setError(null)
    } catch (e) {
      setError(rawError(e))
    } finally {
      setRotating(false)
    }
  }

  useEffect(() => {
    void load(false)
  }, [routineId])

  const row = (label: string, text: string, copy: 'url' | 'key', extra?: ReactNode) => (
    <LabeledRow label={label} className="webhook-row">
      {/* URL e chave são dados técnicos: LTR e isolados, também em árabe. */}
      <Text size="callout" mono lines={2} grow className="routine-webhook-text" dir="ltr">
        {text}
      </Text>
      {extra}
      <IconButton
        title={t(copied === copy ? 'rotinas.webhook.copiado' : 'rotinas.webhook.copiar')}
        icon={copied === copy ? 'checkmark' : 'doc.on.doc'}
        onClick={() => {
          if (!webhook) return
          window.codync.app.copy(copy === 'key' ? webhook.key : (webhook.url ?? webhook.localUrl))
          setCopied(copy)
        }}
      />
    </LabeledRow>
  )

  return (
    <VStack gap={10} className="idioma-superficie" {...superficieIdioma(idioma)}>
      {webhook ? (
        <CardSection
          footer={
            webhook.url
              ? t('rotinas.webhook.ajudaNuvem', { cabecalho: `\u2066${t('rotinas.webhook.cabecalho')}\u2069` })
              : t('rotinas.webhook.ajudaLocal')
          }
        >
          {row(t(webhook.url ? 'rotinas.webhook.url' : 'rotinas.webhook.urlLocal'), webhook.url ?? webhook.localUrl, 'url')}
          {row(
            t('rotinas.webhook.chave'),
            showKey ? webhook.key : '•'.repeat(16),
            'key',
            <>
              <IconButton title={t(showKey ? 'rotinas.webhook.esconder' : 'rotinas.webhook.mostrar')} icon={showKey ? 'eye.slash' : 'eye'} onClick={() => setShowKey((s) => !s)} />
              <IconButton title={t('rotinas.webhook.trocar')} icon="arrow.triangle.2.circlepath" disabled={rotating || store.isOffline} onClick={() => setConfirmRotate(true)} />
            </>,
          )}
        </CardSection>
      ) : error ? (
        <Text size="caption" tone="danger" className="routine-caption">{errorText(error)}</Text>
      ) : (
        <Text size="caption" tone="secondary" className="routine-caption">{t('rotinas.webhook.carregando')}</Text>
      )}
      <Dialog
        open={confirmRotate}
        title={t('rotinas.webhook.trocarPergunta')}
        cancel={t('rotinas.editor.cancelar')}
        language={superficieIdioma(idioma)}
        message={t('rotinas.webhook.trocarAviso')}
        actions={[{ title: t('rotinas.webhook.trocar'), destructive: true, action: () => void load(true) }]}
        onClose={() => setConfirmRotate(false)}
      />
    </VStack>
  )
}
