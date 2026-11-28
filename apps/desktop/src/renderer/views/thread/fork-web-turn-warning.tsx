import { useRef, useState } from 'react'
import { Button } from '../../components/Controls'
import { VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useT } from '../../lib/fork-i18n/estado'

export function WebTurnWarning({ onRetry }: { onRetry: () => Promise<void> }) {
  const t = useT()
  const busy = useRef(false)
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const retry = async () => {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setFailed(false)
    try { await onRetry() } catch { setFailed(true) } finally {
      busy.current = false
      setPending(false)
    }
  }
  return <VStack gap={4} align="start">
    <Text size="caption2" tone="danger" role="status">{t('conversa.mensagens.turnoNaoIniciado')}</Text>
    {failed ? <Text size="caption2" tone="danger" role="alert">{t('conversa.mensagens.falhaAoTentarTurno')}</Text> : null}
    <Button kind="secondary" disabled={pending} onClick={retry}>{t('conversa.mensagens.tentarDeNovo')}</Button>
  </VStack>
}
